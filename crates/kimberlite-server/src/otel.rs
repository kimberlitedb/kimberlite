//! OpenTelemetry integration for distributed tracing.
//!
//! Enabled via the `otel` feature flag. When enabled, the server exports
//! traces to an OTLP-compatible collector (e.g., Jaeger, Grafana Tempo).

use opentelemetry::trace::TracerProvider;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Initializes the OpenTelemetry tracing pipeline.
///
/// Configures an OTLP exporter that sends trace data to the specified endpoint.
/// Falls back to `http://localhost:4317` if no endpoint is provided.
///
/// # Returns
///
/// The `SdkTracerProvider` which must be kept alive for the duration of the program.
/// Dropping it will flush pending spans and shut down the exporter.
///
/// # Errors
///
/// Returns an error if the OTLP exporter or tracing pipeline fails to initialize.
pub fn init_tracing(
    endpoint: Option<&str>,
) -> Result<SdkTracerProvider, Box<dyn std::error::Error>> {
    let otlp_endpoint = endpoint.unwrap_or("http://localhost:4317");

    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(otlp_endpoint)
        .build()?;

    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .build();

    let tracer = provider.tracer("kimberlite-server");
    let telemetry_layer = tracing_opentelemetry::layer().with_tracer(tracer);

    tracing_subscriber::registry()
        .with(telemetry_layer)
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    Ok(provider)
}

/// Shuts down the tracing pipeline, flushing any pending spans.
pub fn shutdown_tracing(provider: SdkTracerProvider) {
    if let Err(e) = provider.shutdown() {
        eprintln!("Error shutting down tracing provider: {e}");
    }
}
