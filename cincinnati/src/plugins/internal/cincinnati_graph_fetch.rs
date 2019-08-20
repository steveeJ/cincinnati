//! This plugin implements fetching a Cincinnati graph via HTTP from a `/v1/graph`-compliant endpoint.

extern crate custom_debug_derive;
extern crate futures;
extern crate quay;
extern crate tokio;

use crate::plugins::{
    AsyncIO, BoxedPlugin, InternalIO, InternalPlugin, InternalPluginWrapper, PluginSettings,
};
use failure::Fallible;
use prometheus::{Counter, Registry};

/// Default URL to upstream graph provider.
pub static DEFAULT_UPSTREAM_URL: &str = "http://localhost:8080/v1/graph";

/// Plugin settings.
#[derive(Clone, CustomDebug, Deserialize, SmartDefault)]
#[serde(default)]
struct CincinnatiGraphFetchSettings {
    #[default(DEFAULT_UPSTREAM_URL.to_string())]
    upstream: String,
}

/// Graph fetcher for Cincinnati `/v1/graph` endpoints.
#[derive(CustomDebug)]
pub struct CincinnatiGraphFetchPlugin {
    /// The upstream from which to fetch the graph
    pub upstream: String,

    /// The optinal metric for counting upstrema requests
    #[debug(skip)]
    pub http_upstream_reqs: Counter,

    /// The optional metric for counting failed upstream requests
    #[debug(skip)]
    pub http_upstream_errors_total: Counter,
}

impl PluginSettings for CincinnatiGraphFetchSettings {
    fn build_plugin(&self, registry: Option<&Registry>) -> Fallible<BoxedPlugin> {
        let cfg = self.clone();
        let plugin = CincinnatiGraphFetchPlugin::try_new(cfg.upstream, registry)?;
        Ok(new_plugin!(InternalPluginWrapper(plugin)))
    }
}

impl CincinnatiGraphFetchPlugin {
    /// Plugin name, for configuration.
    pub const PLUGIN_NAME: &'static str = "cincinnati-graph-fetch";

    /// Validate plugin configuration and fill in defaults.
    pub fn deserialize_config(cfg: toml::Value) -> Fallible<Box<PluginSettings>> {
        let mut settings: CincinnatiGraphFetchSettings = cfg.try_into()?;

        ensure!(!settings.upstream.is_empty(), "empty upstream");

        Ok(Box::new(settings))
    }

    fn try_new(
        upstream: String,
        prometheus_registry: Option<&prometheus::Registry>,
    ) -> Fallible<Self> {
        let http_upstream_reqs = Counter::new(
            "http_upstream_requests_total",
            "Total number of HTTP upstream requests",
        )?;

        let http_upstream_errors_total = Counter::new(
            "http_upstream_errors_total",
            "Total number of HTTP upstream unreachable errors",
        )?;

        if let Some(registry) = &prometheus_registry {
            registry.register(Box::new(http_upstream_reqs.clone()))?;
            registry.register(Box::new(http_upstream_errors_total.clone()))?;
        };

        Ok(Self {
            upstream,
            http_upstream_reqs,
            http_upstream_errors_total,
        })
    }
}

impl InternalPlugin for CincinnatiGraphFetchPlugin {
    fn run_internal(self: &Self, io: InternalIO) -> AsyncIO<InternalIO> {
        use crate::CONTENT_TYPE;
        use actix_web::http::header::{self, HeaderValue};
        use commons::GraphError;
        use futures::{future, Future, Stream};
        use hyper::{Body, Client, Request};

        let upstream = self.upstream.to_owned();

        trace!("getting graph from upstream at {}", upstream);

        // Assemble a request for the upstream Cincinnati service.
        let ups_req = match Request::get(upstream)
            .header(header::ACCEPT, HeaderValue::from_static(CONTENT_TYPE))
            .body(Body::empty())
        {
            Ok(req) => req,
            Err(_) => {
                // TODO: don't mask the error
                return Box::new(future::err(failure::err_msg(
                    GraphError::FailedUpstreamRequest.to_string(),
                )));
            }
        };

        self.http_upstream_reqs.inc();

        let http_upstream_errors_total_failed_request = self.http_upstream_errors_total.clone();
        let http_upstream_errors_total_wrong_status = self.http_upstream_errors_total.clone();

        let future_graph = Client::new()
            .request(ups_req)
            .map_err(move |e| {
                http_upstream_errors_total_failed_request.inc();
                GraphError::FailedUpstreamFetch(e.to_string())
            })
            .and_then(move |res| {
                if res.status().is_success() {
                    future::ok(res)
                } else {
                    // TODO(steveeJ): discuss if this should be a distinct metric
                    http_upstream_errors_total_wrong_status.inc();
                    future::err(GraphError::FailedUpstreamFetch(res.status().to_string()))
                }
            })
            .and_then(|res| {
                res.into_body()
                    .concat2()
                    .map_err(|e| GraphError::FailedUpstreamFetch(e.to_string()))
            })
            .and_then(|body| {
                serde_json::from_slice(&body).map_err(|e| GraphError::FailedJsonIn(e.to_string()))
            })
            .map(|graph| InternalIO {
                graph,
                parameters: io.parameters,
            })
            // TODO: don't mask the error
            .map_err(|e| failure::err_msg(e.to_string()));

        Box::new(future_graph)
    }
}

#[cfg(test)]
mod tests_net {
    use super::*;
    use commons::testing::init_runtime;
    use futures::Future;
    use std::error::Error;
    use tests::generate_custom_graph;

    #[test]
    fn fetch_static_graph_succeeds() -> Result<(), Box<Error>> {
        let mut runtime = init_runtime()?;

        let expected_graph =
            generate_custom_graph(0, 3, Default::default(), Some(vec![(0, 1), (1, 2)]));

        // run mock graph-builder
        let _m = mockito::mock("GET", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&expected_graph)?)
            .create();

        let mut plugin = CincinnatiGraphFetchPlugin::try_new(mockito::server_url(), None)?;
        let http_upstream_reqs = plugin.http_upstream_reqs.clone();
        let http_upstream_errors_total = plugin.http_upstream_errors_total.clone();
        assert_eq!(0, http_upstream_reqs.clone().get() as u64);
        assert_eq!(0, http_upstream_errors_total.clone().get() as u64);

        let future_processed_graph = plugin
            .run_internal(InternalIO {
                graph: Default::default(),
                parameters: Default::default(),
            })
            .and_then(|final_io| Ok(final_io.graph));

        let processed_graph = runtime
            .block_on(future_processed_graph)
            .expect("plugin run failed");

        let expected_graph = crate::Graph::default();
        assert_eq!(expected_graph, processed_graph);

        assert_eq!(1, http_upstream_reqs.get() as u64);
        assert_eq!(0, http_upstream_errors_total.get() as u64);
        Ok(())
    }

    #[test]
    fn fetch_fails_unreachable() -> Result<(), Box<Error>> {
        let mut runtime = init_runtime()?;

        let mut plugin =
            CincinnatiGraphFetchPlugin::try_new("http://not.reachable.test".to_string(), None)?;
        let http_upstream_reqs = plugin.http_upstream_reqs.clone();
        let http_upstream_errors_total = plugin.http_upstream_errors_total.clone();
        assert_eq!(0, http_upstream_reqs.clone().get() as u64);
        assert_eq!(0, http_upstream_errors_total.clone().get() as u64);

        let future_result = plugin
            .run_internal(InternalIO {
                graph: Default::default(),
                parameters: Default::default(),
            })
            .and_then(|final_io| Ok(final_io.graph));

        assert!(runtime.block_on(future_result).is_err());

        assert_eq!(1, http_upstream_reqs.get() as u64);
        assert_eq!(1, http_upstream_errors_total.get() as u64);

        Ok(())
    }

    #[test]
    fn fetch_fails_status_error() -> Result<(), Box<Error>> {
        let mut runtime = init_runtime()?;

        // run mock graph-builder
        let _m = mockito::mock("GET", "/")
            .with_status(404)
            .with_header("content-type", "application/json")
            .with_body("NOT FOUND".to_string())
            .create();

        let plugin = CincinnatiGraphFetchPlugin::try_new(mockito::server_url(), None)?;
        let http_upstream_reqs = plugin.http_upstream_reqs.clone();
        let http_upstream_errors_total = plugin.http_upstream_errors_total.clone();
        assert_eq!(0, http_upstream_reqs.clone().get() as u64);
        assert_eq!(0, http_upstream_errors_total.clone().get() as u64);

        let future_result = plugin
            .run_internal(InternalIO {
                graph: Default::default(),
                parameters: Default::default(),
            })
            .and_then(|final_io| Ok(final_io.graph));

        assert!(runtime.block_on(future_result).is_err());

        assert_eq!(1, http_upstream_reqs.get() as u64);
        assert_eq!(1, http_upstream_errors_total.get() as u64);

        Ok(())
    }

}
