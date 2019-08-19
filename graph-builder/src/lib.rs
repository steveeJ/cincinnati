extern crate actix_web;
extern crate cincinnati;
#[macro_use]
extern crate commons;
extern crate dkregistry;
extern crate env_logger;
#[macro_use]
extern crate failure;
extern crate flate2;
extern crate futures;
extern crate itertools;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;
extern crate prometheus;
extern crate quay;
extern crate regex;
extern crate reqwest;
extern crate semver;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
#[macro_use]
extern crate smart_default;
#[macro_use]
extern crate structopt;
extern crate parking_lot;
extern crate tar;
extern crate tokio;
extern crate toml;

pub mod config;
pub mod graph;
pub mod registry;
pub mod release;
pub mod status;

use std::sync::Arc;

lazy_static! {
    pub static ref SETTINGS: Arc<config::AppSettings> =
        Arc::new(config::AppSettings::assemble().expect("could not assemble AppSettings"));
}
