// Copyright 2018 Alex Crawford
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

extern crate actix_web;
extern crate cincinnati;
extern crate dkregistry;
extern crate env_logger;
extern crate itertools;
#[macro_use]
extern crate failure;
extern crate flate2;
extern crate futures;
#[macro_use]
extern crate log;
extern crate reqwest;
extern crate semver;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
#[macro_use]
extern crate structopt;
extern crate tar;
extern crate tokio_core;

mod config;
mod graph;
mod registry;
mod release;

use actix_web::{http::Method, middleware::Logger, server, App};
use failure::Error;
use log::LevelFilter;
use std::thread;
use structopt::StructOpt;

fn main() -> Result<(), Error> {
    let opts = config::Options::from_args();

    env_logger::Builder::from_default_env()
        .filter(
            Some(module_path!()),
            match opts.verbosity {
                0 => LevelFilter::Warn,
                1 => LevelFilter::Info,
                2 => LevelFilter::Debug,
                _ => LevelFilter::Trace,
            },
        )
        .init();

    let state = graph::State::new();
    let addr = (opts.address, opts.port);

    {
        let state = state.clone();
        thread::spawn(move || graph::run(&opts, &state));
    }

    server::new(move || {
        App::with_state(state.clone())
            .middleware(Logger::default())
            .route("/v1/graph", Method::GET, graph::index)
    })
    .bind(addr)?
    .run();
    Ok(())
}
use self::scopetracker::AllocationTracker;

#[global_allocator]
static GLOBAL: AllocationTracker = AllocationTracker::new();

mod scopetracker {
    use super::GLOBAL;

    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicIsize, Ordering};

    pub struct AllocationTracker {
        mem: AtomicIsize,
    }

    impl AllocationTracker {
        pub const fn new() -> Self {
            AllocationTracker {
                mem: AtomicIsize::new(0),
            }
        }

        fn current_mem(&self) -> isize {
            self.mem.load(Ordering::SeqCst)
        }
    }

    unsafe impl GlobalAlloc for AllocationTracker {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            self.mem.fetch_add(layout.size() as isize, Ordering::SeqCst);
            System.alloc(layout)
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            self.mem.fetch_sub(layout.size() as isize, Ordering::SeqCst);
            System.dealloc(ptr, layout)
        }
    }

    pub struct ScopeTracker<'a> {
        at_start: isize,
        name: &'a str,
        file: &'static str,
        line: u32,
    }

    impl<'a> ScopeTracker<'a> {
        pub fn new(name: &'a str, file: &'static str, line: u32) -> Self {
            Self {
                at_start: GLOBAL.current_mem(),
                name,
                file,
                line,
            }
        }
    }

    impl Drop for ScopeTracker<'_> {
        fn drop(&mut self) {
            let old = self.at_start;
            let new = GLOBAL.current_mem();
            if old != new {
                if self.name == "" {
                    debug!(
                        "{}:{}: {} bytes escaped scope",
                        self.file,
                        self.line,
                        new - old
                    );
                } else {
                    debug!(
                        "{}:{} '{}': {} bytes escaped scope",
                        self.file,
                        self.line,
                        self.name,
                        new - old
                    );
                }
            }
        }
    }

    #[macro_export]
    macro_rules! mem_guard {
        () => {
            mem_guard!("")
        };
        ($e:expr) => {
            let _guard = ScopeTracker::new($e, file!(), line!());
        };
    }
}
