use std::marker::PhantomData;
use std::sync::Arc;

mod acl;
mod gateway;
mod kube;
mod pb;

#[tokio::main]
pub async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    comprehensive::Assembly::<(
        Arc<comprehensive_grpc::server::GrpcServer>,
        PhantomData<gateway::Gateway>,
        Arc<comprehensive_http::diag::HttpServer>,
        PhantomData<comprehensive_spiffe::SpiffeTlsProvider>,
    )>::new()?
    .run()
    .await
}
