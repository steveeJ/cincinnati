//! This plugin implements a dummy for phased rollouts

use crate::Graph;
use failure::Fallible;
use failure::ResultExt;
use plugins::InternalIO;
use plugins::InternalPlugin;
use prometheus_query;
use std::collections::HashMap;

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

static DEFAULT_VERSION_FAILURE_RATIO: &str = "1.0";
static DEFAULT_VERSION_FAILURE_RATIO_THRESHOLD: f64 = 0.8;

impl InternalPlugin for PhasedRolloutPlugin {
    fn run_internal(&self, internal_io: InternalIO) -> Fallible<InternalIO> {
        let (_cluster_id, _version, _channel) =
            match get_multiple_values!(internal_io.parameters, "version", "channel", "id") {
                Ok((cluster_id, version, channel)) => {
                    (cluster_id.clone(), version.clone(), channel.clone())
                }
                Err(e) => bail!(e),
            };

        let mut graph = internal_io.graph;

        // TODO: send a request to tollboth to get information for deriving the answers to:
        // * subscription of this cluster
        // * check the subscription against a map of valid channels
        // * what channels should the update path offer for this cluster?

        // TODO: get this from tollbooth
        let failure_ratio_threshold = None;

        let version_failure_ratios = self.get_failure_ratios()?;
        println!("version_failure_ratios: {:#?}", version_failure_ratios);

        // attach the failure rates to the corresponding releases
        attach_failure_ratios(
            &mut graph,
            version_failure_ratios,
            DEFAULT_VERSION_FAILURE_RATIO.to_string(),
        )?;

        // remove releases above the given failure threshold
        let removed = filter_by_failure_ratio(
            &mut graph,
            failure_ratio_threshold.unwrap_or(DEFAULT_VERSION_FAILURE_RATIO_THRESHOLD),
        )?;
        println!("removed {} releases due to failure ratio", removed);

        println!("graph: {:#?}", graph);
        Ok(InternalIO {
            graph,
            parameters: internal_io.parameters,
        })
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

fn attach_failure_ratios(
    graph: &mut Graph,
    version_failure_ratios: HashMap<String, String>,
    default_version_failure_ratio: String,
) -> Fallible<()> {
    graph.find_by_fn_mut(|release| match release {
        crate::Release::Concrete(concrete_release) => {
            let failure_ratio = match version_failure_ratios.get(&concrete_release.version) {
                Some(failure_ratio) => failure_ratio.to_string(),
                None => {
                    // TODO: discuss how we treat versions without a failure ratio?
                    default_version_failure_ratio.clone()
                }
            };

            concrete_release
                .metadata
                .insert("failure_ratio".to_string(), failure_ratio);

            true
        }
        _ => false,
    });

    Ok(())
}

fn filter_by_failure_ratio(graph: &mut Graph, failure_ratio_threshold: f64) -> Fallible<usize> {
    let to_remove = graph.find_by_fn_mut(|release| match release {
        crate::Release::Concrete(concrete_release) => {
            if let Some(version_failure_ratio) = concrete_release.metadata.get("failure_ratio") {
                version_failure_ratio.parse::<f64>().unwrap() > failure_ratio_threshold
            } else {
                // defensively remove any version without a known failure ratio
                true
            }
        }
        _ => false,
    });

    let removed = graph.remove_releases(
        to_remove
            .iter()
            .map(|(releaseId, _)| releaseId.to_owned())
            .collect(),
    );

    Ok(removed)
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
    fn test_plugin_infogw() -> Fallible<()> {
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
                    5,
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
