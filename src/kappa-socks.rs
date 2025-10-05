use comprehensive::ResourceDependencies;
use std::marker::PhantomData;
use std::sync::Arc;

mod api;
mod convert;
mod encap;
mod http_server;
mod kube;
mod mtls;
mod multiio;
mod pb;
mod router;
mod socks_proto;
mod socks_server;

#[derive(ResourceDependencies)]
struct TopDependencies {
    _socks: Arc<socks_server::SocksServer>,
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
