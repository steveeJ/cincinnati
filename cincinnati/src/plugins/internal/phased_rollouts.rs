//! This plugin implements a dummy for phased rollouts

use failure::Fallible;
use failure::ResultExt;
use plugins::InternalIO;
use plugins::InternalPlugin;
use prometheus_query;
use std::collections::HashMap;
use ReleaseId;

pub struct PhasedRolloutPlugin {
    pub tollbooth_api_base: String,
    pub prometheus_api_base: String,
    pub prometheus_api_token: String,
    pub prometheus_query_override: Option<String>,
}

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

static PROMETHEUS_QUERY_DEFAULT: &str = r#"(
            avg by (version) (count_over_time(cluster_version{type="failure"}[1y]))
            / on (version)
            avg by (version) (count_over_time(cluster_version[1y]))
            )"#;

impl InternalPlugin for PhasedRolloutPlugin {
    fn run_internal(&self, internal_io: InternalIO) -> Fallible<InternalIO> {
        let (_cluster_id, _version, _channel) =
            match get_multiple_values!(internal_io.parameters, "version", "channel", "id") {
                Ok((cluster_id, version, channel)) => {
                    (cluster_id.clone(), version.clone(), channel.clone())
                }
                Err(e) => bail!(e),
            };

        // TODO: send a request to tollboth to get information for deriving the answers to:
        // * subscription of this cluster
        // * check the subscription against a map of valid channels
        // * what channels should the update path offer for this cluster?
        // * what failure rate does this cluster accept for updates?

        // TODO: ask prometheus about the state of all valid releases
        // * are any of the upgrade paths known to fail?
        let prometheus_client = prometheus_query::v1::Client::builder()
            .api_base(Some(self.prometheus_api_base.clone()))
            .access_token(Some(self.prometheus_api_token.clone()))
            .build()
            .context("could not build prometheus client")?;

        use prometheus_query::v1::queries::*;

        let prometheus_query =
            if let Some(prometheus_query_override) = &self.prometheus_query_override {
                prometheus_query_override.to_owned()
            } else {
                PROMETHEUS_QUERY_DEFAULT.to_string()
            };

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

        // TODO: get a HashMap of release:failure_ratio
        let failure_ratios: HashMap<&str, &String> = result
            .into_iter()
            .filter_map(|vector_result: &VectorResult| {
                let (metric, value) = vector_result.get_metric_value_pair();
                let version = if let Some(metric_object) = metric.as_object() {
                    if let Some(version_value) = metric_object.get("version") {
                        if let Some(version_string) = version_value.as_str() {
                            version_string
                        } else {
                            debug!("malformed version '{:?}: not a string", version_value);
                            return None;
                        }
                    } else {
                        debug!("malformed result '{:?}: not an Object", metric);
                        return None;
                    }
                } else {
                    debug!(
                        "malformed result '{:?}: did not find 'version' label in metric",
                        metric
                    );
                    return None;
                };

                let (_, failure_ratio) = value.get_time_sample_pair();

                Some((version, failure_ratio))
            })
            .collect();

        println!("graph: {:?}", internal_io.graph);

        for next_release in internal_io.graph.next_releases(&ReleaseId(0.into())) {
            print!("release: {:?}", next_release);
            // TODO: attach the failure rates to the corresponding releases
        }

        Ok(internal_io)
    }
}

#[cfg(test)]
pub mod tests {
    extern crate env_logger;

    use super::*;
    use plugins::{self, InternalPluginWrapper, Plugin};
    use std::collections::HashMap;
    use try_from::TryInto;

    static ENV_PROMETHEUS_API_TOKEN: &str = "PROMETHEUS_API_TOKEN";

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

    #[cfg(feature = "test-net-private")]
    #[test]
    fn test_plugin() -> Fallible<()> {
        let _ = env_logger::try_init_from_env(env_logger::Env::default());

        let plugin = InternalPluginWrapper(PhasedRolloutPlugin {
            tollbooth_api_base: "".to_string(),
            prometheus_api_base: "https://infogw-data.api.openshift.com".to_string(),
            prometheus_api_token: std::env::var(ENV_PROMETHEUS_API_TOKEN)
                .context(format!("{} not set", ENV_PROMETHEUS_API_TOKEN))?,
            prometheus_query_override: Some(
                r#"(
                    avg by (version) (count_over_time(cluster_version{type="failure"}[1w]))
                    / on (version)
                    avg by (version) (count_over_time(cluster_version[1w]))
                )"#
                .to_string(),
            ),
        });

        plugin.run(
            plugins::InternalIO {
                graph: crate::tests::generate_custom_graph(
                    0,
                    4,
                    Default::default(),
                    None,
                    Some("4.0.0-0.{variable}"),
                ),
                parameters: [("version", "4.0.0-0.1"), ("channel", ""), ("id", "")]
                    .iter()
                    .map(|(key, value)| (key.to_string(), value.to_string()))
                    .collect(),
            }
            .try_into()?,
        )?;

        bail!("not implemented yet")
    }
}
