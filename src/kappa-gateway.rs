use comprehensive::ResourceDependencies;
use std::marker::PhantomData;
use std::sync::Arc;

mod acl;
mod gateway;
mod kube;
mod pb;

#[derive(ResourceDependencies)]
struct TopDependencies {
    _server: Arc<comprehensive_grpc::server::GrpcServer>,
    _gateway: PhantomData<gateway::Gateway>,
    _diag: Arc<comprehensive_http::diag::HttpServer>,
    _spiffe: PhantomData<comprehensive_spiffe::SpiffeTlsProvider>,
}

#[tokio::main]
pub async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    comprehensive::Assembly::<TopDependencies>::new()?
        .run()
        .await
}
