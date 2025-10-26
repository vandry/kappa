use std::marker::PhantomData;
use std::sync::Arc;

mod acl;
mod gateway;
mod kube;
mod pb;

#[tokio::main]
pub async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    comprehensive::Assembly::<(
        Arc<comprehensive_grpc::server::GrpcServer>,
        PhantomData<gateway::Gateway>,
        Arc<comprehensive_http::diag::HttpServer>,
        PhantomData<comprehensive_spiffe::SpiffeTlsProvider>,
    )>::new()?
    .run()
    .await
}
