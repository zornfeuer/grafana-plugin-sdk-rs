use http::HeaderMap;
use opentelemetry::{global, propagation::Extractor, Context};
use tracing_opentelemetry::OpenTelemetrySpanExt as _;

struct HeaderExtractor<'a>(&'a HeaderMap);

impl Extractor for HeaderExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|value| value.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(http::HeaderName::as_str).collect()
    }
}

fn extract(headers: &HeaderMap) -> Context {
    global::get_text_map_propagator(|propagator| propagator.extract(&HeaderExtractor(headers)))
}

pub(crate) fn set_parent_from_headers(span: &tracing::Span, headers: &HeaderMap) {
    let _ = span.set_parent(extract(headers));
}

#[cfg(test)]
mod tests {
    use opentelemetry::trace::TraceContextExt as _;

    use super::*;

    #[test]
    fn extracts_w3c_trace_context_from_grpc_metadata() {
        global::set_text_map_propagator(
            opentelemetry_sdk::propagation::TraceContextPropagator::new(),
        );
        let mut headers = HeaderMap::new();
        headers.insert(
            "traceparent",
            "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01"
                .parse()
                .unwrap(),
        );
        headers.insert("tracestate", "vendor=value".parse().unwrap());

        let context = extract(&headers);
        let span = context.span();
        let span_context = span.span_context();

        assert!(span_context.is_remote());
        assert_eq!(
            span_context.trace_id().to_string(),
            "0af7651916cd43dd8448eb211c80319c"
        );
        assert_eq!(span_context.span_id().to_string(), "b7ad6b7169203331");
        assert_eq!(span_context.trace_state().header(), "vendor=value");
    }

    #[test]
    fn invalid_trace_context_is_ignored() {
        global::set_text_map_propagator(
            opentelemetry_sdk::propagation::TraceContextPropagator::new(),
        );
        let mut headers = HeaderMap::new();
        headers.insert("traceparent", "invalid".parse().unwrap());

        assert!(!extract(&headers).span().span_context().is_valid());
    }
}
