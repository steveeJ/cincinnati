//! Implements calls to the /v1/query endpoint

use super::Client;
use futures::future::Future;
use serde::Deserialize;

mod instant;

#[derive(Deserialize, Debug, PartialEq)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum QueryResult {
    Success(QuerySuccess),
    Error(QueryError),
}

#[derive(Deserialize, Debug, PartialEq)]
pub struct QuerySuccess {
    data: QueryData,
    warnings: Option<Vec<String>>,
}

impl QuerySuccess {
    pub fn data(&self) -> &QueryData {
        &self.data
    }
}

#[derive(Deserialize, Debug, PartialEq)]
pub struct QueryError {
    data: serde_json::Value,
    #[serde(rename = "errorType")]
    error_type: String,
    error: String,
    warnings: Option<Vec<String>>,
}

#[derive(Deserialize, Debug, PartialEq)]
#[serde(tag = "resultType", content = "result", rename_all = "lowercase")]
pub enum QueryData {
    Matrix(Vec<Vec<VectorResult>>),
    Vector(Vec<VectorResult>),
    // TODO(steveeJ): add Scalar
    // TODO(steveeJ): add String
}

#[derive(Deserialize, Debug, PartialEq)]
pub struct VectorResult {
    metric: serde_json::Value,
    value: VectorValue,
}

impl VectorResult {
    /// Get a tuple of borrows to the metric and value
    pub fn get_metric_value_pair(&self) -> (&serde_json::Value, &VectorValue) {
        (&self.metric, &self.value)
    }
}

#[derive(Default, Deserialize, Debug, PartialEq)]
pub struct VectorValue {
    time: f64,
    sample: String,
}

impl VectorValue {
    pub fn get_time_sample_pair(&self) -> (&f64, &String) {
        (&self.time, &self.sample)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use failure::Fallible;

    #[test]
    fn deserialize_queryresult() -> Fallible<()> {
        let query_result_str = r#"{
            "status": "success",
            "data": {
                "resultType": "vector",
                "result": [
                    {
                        "metric": { "version": "4.0.0-0.alpha-2019-03-05-054505" },
                        "value": [ 1551992754.228, "12415917818" ]
                    },
                    {
                        "metric": { "version": "4.0.0-0.7" },
                        "value": [ 1551992754.228, "13967876561" ]
                    }
                ]
            },
            "warnings": [ "just a test warning" ]
        }"#;

        let expected_result = QuerySuccess {
            data: QueryData::Vector(vec![
                VectorResult {
                    metric: json!({ "version": "4.0.0-0.alpha-2019-03-05-054505" }),
                    value: VectorValue {
                        time: 1551992754.228,
                        sample: "12415917818".to_string(),
                    },
                },
                VectorResult {
                    metric: json!({ "version": "4.0.0-0.7" }),
                    value: VectorValue {
                        time: 1551992754.228,
                        sample: "13967876561".to_string(),
                    },
                },
            ]),
            warnings: Some(vec!["just a test warning".to_string()]),
        };

        match serde_json::from_str::<QueryResult>(query_result_str)? {
            QueryResult::Success(query_success) => assert_eq!(expected_result, query_success),
            _ => bail!("expected success"),
        };

        Ok(())
    }
}
