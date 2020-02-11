pub mod command;
pub mod nodes;

pub mod prelude {
    pub use anyhow::{bail, ensure, Context, Error, Result};
    pub use log::{debug, error, info, trace, warn};
}

use prelude::*;

use nodes::downloader::Downloader;
use nodes::persistence::Persistence;

#[paw::main]
fn main(args: command::Args) -> Result<()> {
    env_logger::init();

    match args.cmd {
        command::Command::DownloadNodes(cmd) => {
            let persistence =
                Persistence::new(args.nodes_persistence_dir, cmd.persistence_mode.clone())?;

            let mut downloader = Downloader {
                options: cmd,
                persistence,
            };

            tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(downloader.download())?;
        }
        unhandled => println!("Command not handled: {:#?}", unhandled),
    }

    Ok(())
}
