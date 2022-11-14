extern crate bincode;
extern crate core;
extern crate libc;
#[macro_use]
extern crate log;
extern crate once_cell;
extern crate rustler;
extern crate scylla;
extern crate serde;
extern crate siphasher;
extern crate tokio;

mod atoms;
mod client;
mod nif;
mod task;

rustler::init!(
    "poseidon_nif",
    [client::start, client::deliver, client::stop],
    load = nif::on_load
);
