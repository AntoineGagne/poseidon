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

pub struct Duration(std::time::Duration);

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

pub enum Consistency {
    Any,
    One,
    Two,
    Three,
    Quorum,
    All,
    LocalQuorum,
    EachQuorum,
    LocalOne,
}

impl<'a> Decoder<'a> for Consistency {
    fn decode(term: Term<'a>) -> NifResult<Self> {
        let atom: Atom = term.decode()?;
        match atom {
            any if any == crate::atoms::any() => Ok(Consistency::Any),
            one if one == crate::atoms::one() => Ok(Consistency::One),
            two if two == crate::atoms::two() => Ok(Consistency::Two),
            three if three == crate::atoms::three() => Ok(Consistency::Three),
            quorum if quorum == crate::atoms::quorum() => Ok(Consistency::Quorum),
            all if all == crate::atoms::all() => Ok(Consistency::All),
            local_quorum if local_quorum == crate::atoms::local_quorum() => Ok(Consistency::LocalQuorum),
            each_quorum if each_quorum == crate::atoms::each_quorum() => Ok(Consistency::EachQuorum),
            local_one if local_one == crate::atoms::local_one() => Ok(Consistency::LocalOne),
            _ => Err(rustler::Error::BadArg),
        }
    }
}

impl From<&scylla::frame::types::Consistency> for Consistency {
    fn from(consistency: &scylla::frame::types::Consistency) -> Consistency {
        use scylla::frame::types;
        match consistency {
            types::Consistency::Any => Consistency::Any,
            types::Consistency::One => Consistency::One,
            types::Consistency::Two => Consistency::Two,
            types::Consistency::Three => Consistency::Three,
            types::Consistency::Quorum => Consistency::Quorum,
            types::Consistency::All => Consistency::All,
            types::Consistency::LocalQuorum => Consistency::LocalQuorum,
            types::Consistency::EachQuorum => Consistency::EachQuorum,
            types::Consistency::LocalOne => Consistency::LocalOne,
        }
    }
}

impl<'a> Encoder for Consistency {
    fn encode<'b>(&self, env: Env<'b>) -> Term<'b> {
        match self {
            Consistency::Any => crate::atoms::any(),
            Consistency::One => crate::atoms::one(),
            Consistency::Two => crate::atoms::two(),
            Consistency::Three => crate::atoms::three(),
            Consistency::Quorum => crate::atoms::quorum(),
            Consistency::All => crate::atoms::all(),
            Consistency::LocalQuorum => crate::atoms::local_quorum(),
            Consistency::EachQuorum => crate::atoms::each_quorum(),
            Consistency::LocalOne => crate::atoms::local_one(),
        }.encode(env)
    }
}

#[derive(NifStruct)]
#[module = "Poseidon.Client.Config"]
pub struct Config {
    pub uri: String,
    pub request_timeout: Option<Duration>,
    pub default_consistency: Option<Consistency>,
    pub connection_timeout: Option<Duration>,
    pub fetch_schema_metadata: Option<bool>,
    pub keepalive_interval: Option<Duration>,
    pub auto_schema_agreement_timeout: Option<Duration>,
    pub refresh_metadata_on_auto_schema_agreement: Option<bool>
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
