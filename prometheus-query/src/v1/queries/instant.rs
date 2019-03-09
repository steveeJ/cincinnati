//! Impelement instant queries

use super::{Client, Future, QueryResult};
use failure::Error;
use reqwest;
use std::time::Duration;

pub static INSTANT_QUERY_PATH_SUFFIX: &str = "/api/v1/query";

impl Client {
    /// Sends the given query to the remote API, given an optional `time` and timeout.SystemTime
    ///
    /// The `time` is measured since the UNIX_EPOCH
    pub fn query(
        &self,
        query: String,
        time: Option<chrono::DateTime<chrono::Utc>>,
        timeout: Option<Duration>,
    ) -> impl Future<Item = QueryResult, Error = Error> + '_ {
        futures::future::result(self.new_request(reqwest::Method::GET, INSTANT_QUERY_PATH_SUFFIX))
            .and_then(move |request_builder| {
                let mut query = vec![("query", query)];

                if let Some(time) = time {
                    query.push(("time", time.to_rfc3339()));
                }

                if let Some(timeout) = timeout {
                    query.push(("timeout", format!("{}s", timeout.as_secs())));
                };

                trace!("sending query '{:?}'", &query);
                request_builder.query(&query).send().map_err(Into::into)
            })
            .and_then(|response| response.error_for_status().map_err(Into::into))
            .and_then(|mut response| response.json().map_err(Into::into))
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use failure::{Fallible, ResultExt};

    #[cfg(feature = "test-net-private")]
    #[ignore]
    #[test]
    fn query_infogw() -> Fallible<()> {
        let _ = env_logger::try_init_from_env(env_logger::Env::default());

        let token =
            std::env::var("PROMETHEUS_API_TOKEN").context("PROMETHEUS_API_TOKEN not set")?;

        let client = Client::builder()
            .api_base(Some("https://infogw-data.api.openshift.com".to_string()))
            .access_token(Some(token))
            .build()?;

        let query = r#"avg by (version) (count by (version) (cluster_version{type="failure"}) / on (version) count by (version) (cluster_version))"#;

        let expected_json = r#"
        {"status":"success","data":{"resultType":"vector","result":[{"metric":{"version":"4.0.0-0.3"},"value":[1552056334,"0.04081632653061224"]},{"metric":{"version":"4.0.0-0.alpha-2019-03-05-054505"},"value":[1552056334,"0.056451612903225805"]},{"metric":{"version":"4.0.0-0.6"},"value":[1552056334,"0.07692307692307693"]},{"metric":{"version":"4.0.0-0.7"},"value":[1552056334,"0.04519774011299435"]},{"metric":{"version":"4.0.0-0.5"},"value":[1552056334,"0.0625"]}]}}
        "#;

        let mut expected_result: Vec<VectorResult> =
            match serde_json::from_str::<QueryResult>(expected_json)? {
                QueryResult::Success(query_success) => match query_success.data {
                    QueryData::Vector(vector) => vector,
                    _ => bail!("expected vector"),
                },
                _ => bail!("expected result"),
            };

        let mut result: Vec<VectorResult> = match tokio::runtime::current_thread::Runtime::new()
            .unwrap()
            .block_on(client.query(
                query.to_string(),
                Some(chrono::DateTime::from_utc(
                    chrono::NaiveDateTime::from_timestamp(1_552_056_334, 675 * 100),
                    chrono::Utc,
                )),
                None,
            ))? {
            QueryResult::Success(query_success) => match query_success.data {
                QueryData::Vector(vector) => vector,
                _ => bail!("expected vector"),
            },
            _ => bail!("expected result"),
        };

        fn sort_by_version(a: &VectorResult, b: &VectorResult) -> std::cmp::Ordering {
            let a = a
                .metric
                .as_object()
                .unwrap()
                .get("version")
                .unwrap()
                .as_str()
                .unwrap();
            let b = b
                .metric
                .as_object()
                .unwrap()
                .get("version")
                .unwrap()
                .as_str()
                .unwrap();

            if a < b {
                std::cmp::Ordering::Less
            } else if a > b {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        }

        expected_result.sort_by(sort_by_version);
        result.sort_by(sort_by_version);

        assert_eq!(expected_result, result);

        Ok(())
    }
}
