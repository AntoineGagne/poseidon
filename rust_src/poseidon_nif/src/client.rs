use std::sync::{Arc, Mutex};

use rustler::{Atom, Encoder, Env, NifStruct, OwnedEnv, ResourceArc};
use rustler::types::Pid;
use scylla::transport::load_balancing::{DcAwareRoundRobinPolicy, TokenAwarePolicy};
use scylla::{IntoTypedRows, Session, SessionBuilder};
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

#[derive(NifStruct)]
#[module = "Poseidon.Client.Config"]
pub struct Config {}

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

fn spawn_client(global_env: Env, _config: Config, mut receiver: Receiver<Action>) {
    let pid = global_env.pid();
    task::spawn(async move {
        let mut env = OwnedEnv::new();

        let dc_robin = Box::new(DcAwareRoundRobinPolicy::new("us_east".to_string()));
        let policy = Arc::new(TokenAwarePolicy::new(dc_robin));
        match SessionBuilder::new()
            .known_node("127.0.0.1:9042")
            .load_balancing(policy)
            .build().await {
                Ok(session) => {
                    info!("Opened session to \"{}\" with DC \"{}\"", "127.0.0.1:9042", "us_east");
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
                                    .prepare(query)
                                    .await {
                                        Ok(prepared) => {
                                            env.send_and_clear(&pid, move |env| {
                                                (ok(), PreparedQuery::new(prepared)).encode(env)
                                            });
                                        },
                                        Err(error) => {
                                            env.send_and_clear(&pid, move |env| {
                                                (crate::atoms::error(), error.to_string()).encode(env)
                                            })
                                        }
                                    }
                            },
                            Some(Action::Stop) => break,
                            None => continue,
                        }
                    }
                },
                Err(error) => {
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
