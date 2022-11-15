use std::sync::{Arc, Mutex};

use rustler::{Atom, Encoder, Env, NifStruct, OwnedEnv, ResourceArc, Decoder, NifResult, Term};
use rustler::types::Pid;
use scylla::transport::load_balancing::{DcAwareRoundRobinPolicy, TokenAwarePolicy};
use scylla::SessionBuilder;
use scylla::prepared_statement::PreparedStatement;
use tokio::sync::mpsc::{channel, Receiver, Sender};

use crate::atoms::ok;
use crate::task;

pub struct Reference(Mutex<Sender<Action>>);

impl Reference {
    fn new(tx: Sender<Action>) -> ResourceArc<Reference> {
        ResourceArc::new(Reference(Mutex::new(tx)))
    }
}

pub struct PreparedQuery(Mutex<PreparedStatement>);

impl PreparedQuery {
    fn new(query: PreparedStatement) -> ResourceArc<PreparedQuery> {
        ResourceArc::new(PreparedQuery(Mutex::new(query)))
    }
}

#[derive(NifStruct)]
#[module = "Poseidon.Message"]
struct Message {}

enum Action {
    Send(Pid, Message),
    PrepareQuery(Pid, String),
    Stop,
}

struct Duration(std::time::Duration);

impl<'a> Decoder<'a> for Duration {
    fn decode(term: Term<'a>) -> NifResult<Self> {
        let duration: u64 = term.decode()?;
        Ok(Duration(std::time::Duration::from_millis(duration)))
    }
}

impl<'a> Encoder for Duration {
    fn encode<'b>(&self, env: Env<'b>) -> Term<'b> {
        let duration: u64 = self.0.as_millis().try_into().unwrap();
        duration.encode(env)
    }
}

struct Consistency(scylla::frame::types::Consistency);

impl<'a> Decoder<'a> for Consistency {
    fn decode(term: Term<'a>) -> NifResult<Self> {
        use scylla::frame::types;
        let atom: Atom = term.decode()?;
        match atom {
            any if any == crate::atoms::any() => Ok(Consistency(types::Consistency::Any)),
            one if one == crate::atoms::one() => Ok(Consistency(types::Consistency::One)),
            two if two == crate::atoms::two() => Ok(Consistency(types::Consistency::Two)),
            three if three == crate::atoms::three() => Ok(Consistency(types::Consistency::Three)),
            quorum if quorum == crate::atoms::quorum() => Ok(Consistency(types::Consistency::Quorum)),
            all if all == crate::atoms::all() => Ok(Consistency(types::Consistency::All)),
            local_quorum if local_quorum == crate::atoms::local_quorum() => Ok(Consistency(types::Consistency::LocalQuorum)),
            each_quorum if each_quorum == crate::atoms::each_quorum() => Ok(Consistency(types::Consistency::One)),
            local_one if local_one == crate::atoms::local_one() => Ok(Consistency(types::Consistency::LocalOne)),
            _ => Err(rustler::Error::BadArg),
        }
    }
}

impl<'a> Encoder for Consistency {
    fn encode<'b>(&self, env: Env<'b>) -> Term<'b> {
        use scylla::frame::types;
        match self.0 {
            types::Consistency::Any => crate::atoms::any(),
            types::Consistency::One => crate::atoms::one(),
            types::Consistency::Two => crate::atoms::two(),
            types::Consistency::Three => crate::atoms::three(),
            types::Consistency::Quorum => crate::atoms::quorum(),
            types::Consistency::All => crate::atoms::all(),
            types::Consistency::LocalQuorum => crate::atoms::local_quorum(),
            types::Consistency::EachQuorum => crate::atoms::each_quorum(),
            types::Consistency::LocalOne => crate::atoms::local_one(),
        }.encode(env)
    }
}

#[derive(NifStruct)]
#[module = "Poseidon.Client.Config"]
pub struct Config {
    uri: String,
    request_timeout: Option<Duration>,
    default_consistency: Option<Consistency>,
    connection_timeout: Option<Duration>,
    fetch_schema_metadata: Option<bool>,
    keepalive_interval: Option<Duration>,
    auto_schema_agreement_timeout: Option<Duration>,
    refresh_metadata_on_auto_schema_agreement: Option<bool>
}

pub fn load(env: Env) -> bool {
    rustler::resource!(Reference, env);
    rustler::resource!(PreparedQuery, env);
    true
}

#[rustler::nif(name = "client_start")]
pub fn start<'a>(env: Env<'a>, config: Config) -> (Atom, ResourceArc<Reference>) {
    let (tx, rx) = channel::<Action>(1000);
    spawn_client(env, config, rx);
    (ok(), Reference::new(tx))
}

#[rustler::nif(name = "client_prepare_query")]
pub fn prepare_query<'a>(env: Env<'a>, resource: ResourceArc<Reference>, query: String) -> Atom {
    send(resource, Action::PrepareQuery(env.pid(), query));
    ok()
}

#[rustler::nif(name = "client_send")]
fn deliver<'a>(env: Env<'a>, resource: ResourceArc<Reference>, message: Message) -> (Atom, ResourceArc<Reference>) {
    send(resource.clone(), Action::Send(env.pid(), message));
    (ok(), resource)
}

#[rustler::nif(name = "client_stop")]
pub fn stop(resource: ResourceArc<Reference>) -> Atom {
    send(resource, Action::Stop);
    ok()
}

fn spawn_client(global_env: Env, config: Config, mut receiver: Receiver<Action>) {
    let pid = global_env.pid();
    task::spawn(async move {
        let mut env = OwnedEnv::new();

        let dc_robin = Box::new(DcAwareRoundRobinPolicy::new("us_east".to_string()));
        let policy = Arc::new(TokenAwarePolicy::new(dc_robin));
        match SessionBuilder::new()
            .known_node(&config.uri)
            .load_balancing(policy)
            .build().await {
                Ok(session) => {
                    info!("Opened session to \"{}\"", &config.uri);
                    env.send_and_clear(&pid, move |env| {
                        ok().encode(env)
                    });
                    loop {
                        match receiver.recv().await {
                            Some(Action::Send(pid, _message)) => {
                                env.send_and_clear(&pid, move |env| {
                                    ok().encode(env)
                                });
                            }
                            Some(Action::PrepareQuery(pid, query)) => {
                                match session
                                    .prepare(query.clone())
                                    .await {
                                        Ok(prepared) => {
                                            env.send_and_clear(&pid, move |env| {
                                                (ok(), PreparedQuery::new(prepared)).encode(env)
                                            });
                                        },
                                        Err(error) => {
                                            error!("Failed to prepare query \"{}\" with error {}", &query, error.to_string());
                                            env.send_and_clear(&pid, move |env| {
                                                (crate::atoms::error(), error.to_string()).encode(env)
                                            })
                                        }
                                    }
                            },
                            Some(Action::Stop) => {
                                info!("Closing session to \"{}\"", &config.uri);
                                break
                            },
                            None => continue,
                        }
                    }
                },
                Err(error) => {
                    error!("Failed to open new session to {} with error {}", &config.uri, error.to_string());
                    env.send_and_clear(&pid, move |env| {
                        (crate::atoms::error(), error.to_string()).encode(env)
                    });
                }

            }
    });
}


fn send(resource: ResourceArc<Reference>, action: Action) {
    let lock = resource.0.lock().expect("Failed to obtain a lock");
    let sender = lock.clone();

    task::spawn(async move {
        match sender.send(action).await {
            Ok(_) => (),
            Err(_err) => trace!("send error"),
        }
    });
}
