use clap::Parser;
use opentelemetry::global;
use shuttle_deployer::{
    start, start_proxy, AbstractProvisionerFactory, Args, DeployLayer, Persistence,
    RuntimeLoggerFactory,
};
use tokio::select;
use tonic::transport::Endpoint;
use tracing::trace;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{fmt, EnvFilter};

// The `multi_thread` is needed to prevent a deadlock in shuttle_service::loader::build_crate() which spawns two threads
// Without this, both threads just don't start up
#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let args = Args::parse();

    trace!(args = ?args, "parsed args");

    global::set_text_map_propagator(opentelemetry_datadog::DatadogPropagator::new());

    let fmt_layer = fmt::layer();
    let filter_layer = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info"))
        .unwrap();

    let (persistence, _) = Persistence::new(&args.state).await;
    let tracer = opentelemetry_datadog::new_pipeline()
        .with_service_name("deployer")
        .with_agent_endpoint("http://datadog-agent:8126")
        .install_batch(opentelemetry::runtime::Tokio)
        .unwrap();
    let opentelemetry = tracing_opentelemetry::layer().with_tracer(tracer);
    tracing_subscriber::registry()
        .with(DeployLayer::new(persistence.clone()))
        .with(filter_layer)
        .with(fmt_layer)
        .with(opentelemetry)
        .init();

    let provisioner_uri = Endpoint::try_from(format!(
        "http://{}:{}",
        args.provisioner_address, args.provisioner_port
    ))
    .expect("provisioner uri is not valid");

    let abstract_factory =
        AbstractProvisionerFactory::new(provisioner_uri, persistence.clone(), persistence.clone());

    let runtime_logger_factory = RuntimeLoggerFactory::new(persistence.get_log_sender());

    select! {
        _ = start_proxy(args.proxy_address, args.proxy_fqdn.clone(), persistence.clone()) => {},
        _ = start(abstract_factory, runtime_logger_factory, persistence, args) => {},
    }
}
