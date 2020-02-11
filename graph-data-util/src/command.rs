use std::path::PathBuf;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
pub struct Args {
    #[structopt(long, default_value = ".nodes", help = "Path to cache the node files")]
    pub nodes_persistence_dir: PathBuf,

    #[structopt(subcommand)]
    pub cmd: Command,
}

#[derive(Debug, StructOpt)]
pub enum Command {
    DownloadNodes(download_nodes::DownloadNodes),
    PushToQuay(push_to_quay::PushToQuay),
}

pub mod download_nodes {
    use crate::nodes::downloader::DownloadMode;
    use std::path::PathBuf;
    use structopt::StructOpt;

    #[derive(Debug, StructOpt)]
    pub struct DownloadNodes {
        #[structopt(long, default_value = "quay.io")]
        pub registry: String,

        #[structopt(long, default_value = "openshift-release-dev/ocp-release")]
        pub repository: String,


        #[structopt(
                long,
                default_value = "VerifyExistingAddNew",
                possible_values = &DownloadMode::variants(),
                case_insensitive = true,
                help = "Sets the persistence mode for the node download. (case insensitive)",
            )]
        pub persistence_mode: DownloadMode,

        #[structopt(long, default_value = "16")]
        pub concurrency: usize,

        #[structopt(
            long,
            default_value = "io.openshift.upgrades.graph.release.manifestref"
        )]
        pub manifestref_key: String,

        #[structopt(long)]
        pub username: Option<String>,

        #[structopt(long)]
        pub password: Option<String>,
    }
}


pub mod push_to_quay {
    use std::path::PathBuf;
    use structopt::StructOpt;

    #[derive(Debug)]
    pub struct Versions(Vec<String>);

    impl std::str::FromStr for Versions {
        type Err = crate::Error;

        fn from_str(s: &str) -> crate::Result<Self> {
            Ok(Self(s.split(',').map(std::string::String::from).collect()))
        }
    }

    #[derive(Debug, StructOpt)]
    pub struct PushToQuay {
        #[structopt(long, help = "Directory containing the metadata input")]
        pub metadata_input_dir: PathBuf,

        #[structopt(
            long,
            help = "File containing a Quay token (https://docs.quay.io/api/#applications-and-tokens)"
        )]
        pub token_file: Option<PathBuf>,

        #[structopt(long, help = "Comma Seperated Versions to sync")]
        pub versions: Option<Versions>,
    }
}
