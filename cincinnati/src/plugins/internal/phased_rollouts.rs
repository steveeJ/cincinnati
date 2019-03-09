//! This plugin implements a dummy for phased rollouts

use failure::Fallible;
use failure::ResultExt;
use plugins::InternalIO;
use plugins::InternalPlugin;
use prometheus_query;

pub struct PhasedRolloutPlugin {
    pub tollbooth_api_base: String,
    pub prometheus_api_base: String,
}

pub static ENV_PROMETHEUS_API_TOKEN: &str = "PROMETHEUS_API_TOKEN";
pub static ENV_PROMETHEUS_QUERY: &str = "PROMETHEUS_QUERY";

// TODO: move this somewhere common
macro_rules! get_multiple_values {
    ($map:expr, $( $key:expr ),* ) => {
        {
            let closure = || {
                Ok(
                    (
                        $(
                            if let Some(value) = $map.get($key) {
                                value
                            } else {
                                bail!("could not find key '{}'", $key)
                            },
                        )*
                    )
                )
            };
            closure()
        }
    }
}

impl InternalPlugin for PhasedRolloutPlugin {
    fn run_internal(&self, internal_io: InternalIO) -> Fallible<InternalIO> {
        let (cluster_id, version, channel) =
            get_multiple_values!(internal_io.parameters, "version", "channel", "id")?;

        /// TODO: send a request to tollboth to get information for deriving the answers to:
        /// * subscription of this cluster
        /// * check the subscription against a map of valid channels
        /// * what channels should the update path offer for this cluster?
        /// * what failure rate does this cluster accept for updates?

        /// TODO: ask prometheus about the state of all valid releases
        /// * are any of the upgrade paths known to fail?
        let prometheus_token = std::env::var(ENV_PROMETHEUS_API_TOKEN)
            .context(format!("{} not set", ENV_PROMETHEUS_API_TOKEN))?;

        let prometheus_client = prometheus_query::v1::Client::builder()
            .api_base(Some(self.prometheus_api_base.clone()))
            .access_token(Some(prometheus_token))
            .build()
            .context("could not build prometheus client")?;

        use prometheus_query::{chrono, v1::queries::*};

        let prometheus_query =
            r#"avg by (version) (count by (version) (cluster_version{type="failure"}) / on (version) count by (version) (cluster_version))"#.to_string();

        let result: QuerySuccess = match tokio::runtime::current_thread::Runtime::new()
            .unwrap()
            .block_on(prometheus_client.query(prometheus_query, None, None))?
        {
            QueryResult::Success(query_success) => query_success,
            _ => bail!("expected result"),
        };

        let result: &Vec<VectorResult> = match result.data() {
            QueryData::Vector(ref vector) => vector,
            _ => bail!("expected vector"),
        };

        /// TODO: attach the failure rates to the corresponding releases
        bail!("phased rollout plugin is not implemented yet")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // TODO: move this somewhere common
    #[test]
    fn ensure_get_multiple_values() {
        let params = [
            ("a".to_string(), "a".to_string()),
            ("b".to_string(), "b".to_string()),
        ]
        .iter()
        .cloned()
        .collect::<HashMap<String, String>>();

        let (a, b): (&String, &String) = get_multiple_values!(params, "a", "b").unwrap();
        assert_eq!((&"a".to_string(), &"b".to_string()), (a, b));

        let params = [
            ("a".to_string(), "a".to_string()),
            ("b".to_string(), "b".to_string()),
        ]
        .iter()
        .cloned()
        .collect::<HashMap<String, String>>();

        assert!(get_multiple_values!(params, "c").is_err());
    }
}
