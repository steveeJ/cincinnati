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

static PROMETHEUS_QUERY_DEFAULT: &str = r#"(
        count by (version) (count_over_time(cluster_version{type="failure"}[14d]))
            / on (version)
        count by (version) (count_over_time(cluster_version[14d]))
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

        let version_failure_ratio = self.get_failure_ratios()?;
        println!("version_failure_ratio: {:#?}", version_failure_ratio);

        // TODO: attach the failure rates to the corresponding releases
        let mut graph = internal_io.graph;
        let _ = graph.find_by_fn_mut(|release| match release {
            crate::Release::Concrete(concrete_release) => {
                if let Some(failure_ratio) = version_failure_ratio.get(&concrete_release.version) {
                    concrete_release.metadata.insert("failure_ratio".to_string(),failure_ratio.to_string());
                    true
                } else {
                    false
                }
            }
            _ => false,
        });

        println!("graph: {:#?}", graph);
        Ok(InternalIO{
                graph,
                parameters: internal_io.parameters,
            }
        )
    }
}

impl PhasedRolloutPlugin {
    fn get_failure_ratios(&self) -> Fallible<HashMap<String, String>> {
        use prometheus_query::v1::queries::*;

        let prometheus_client = prometheus_query::v1::Client::builder()
            .api_base(Some(self.prometheus_api_base.clone()))
            .access_token(Some(self.prometheus_api_token.clone()))
            .build()
            .context("could not build prometheus client")?;

        let prometheus_query =
            if let Some(prometheus_query_override) = &self.prometheus_query_override {
                prometheus_query_override.to_owned()
            } else {
                PROMETHEUS_QUERY_DEFAULT.to_string()
            };

        let result: QuerySuccess = match tokio::runtime::current_thread::Runtime::new()
            .context("current_thread::Runtime::new() failed")?
            .block_on(prometheus_client.query(prometheus_query, None, None))?
        {
            QueryResult::Success(query_success) => query_success,
            _ => bail!("expected result"),
        };

        let result: &Vec<VectorResult> = match result.data() {
            QueryData::Vector(ref vector) => vector,
            _ => bail!("expected vector"),
        };

        Ok(result
            .iter()
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

                Some((version.to_owned(), failure_ratio.to_owned()))
            })
            .collect())
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
                    sum by (version) (count_over_time(cluster_version{type="failure"}[14d]))
                        / on (version)
                    sum by (version) (count_over_time(cluster_version[14d]))
                )"#
                .to_string(),
            ),
        });

        let io = plugin.run(
            plugins::InternalIO {
                graph: crate::tests::generate_custom_graph(
                    9,
                    3,
                    Default::default(),
                    None,
                    Some("4.0.0-0.{variable}"),
                ),
                parameters: [("version", "4.0.0-0.9"), ("channel", ""), ("id", "")]
                    .iter()
                    .map(|(key, value)| (key.to_string(), value.to_string()))
                    .collect(),
            }
            .try_into()?,
        )?;

        bail!("not implemented yet")
    }
}
