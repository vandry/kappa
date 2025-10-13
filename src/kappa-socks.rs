use std::marker::PhantomData;
use std::sync::Arc;

mod api;
mod convert;
mod encap;
mod endpoint;
mod error_server;
mod http_server;
mod kube;
mod mtls;
mod multiio;
mod pb;
mod router;
mod socks_proto;
mod socks_server;

#[tokio::main]
pub async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    comprehensive::Assembly::<(
        Arc<socks_server::SocksServer>,
        Arc<comprehensive_http::diag::HttpServer>,
        PhantomData<comprehensive_spiffe::SpiffeTlsProvider>,
    )>::new()?
    .run()
    .await
}
