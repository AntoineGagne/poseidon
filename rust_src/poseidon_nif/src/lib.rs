extern crate bincode;
extern crate core;
extern crate libc;
extern crate rustler;
extern crate serde;
extern crate siphasher;

mod atoms;
mod nif;

rustler::init!("poseidon_nif", [nif::start], load = nif::on_load);
