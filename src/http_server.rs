use axum::Router;
use axum::response::{Html, IntoResponse};
use comprehensive::v1::{AssemblyRuntime, Resource, resource};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder;
use hyper_util::service::TowerToHyperService;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};

pub struct HttpServer {
    builder: Builder<TokioExecutor>,
    app: Router,
}

async fn root_page() -> impl IntoResponse {
    const ROOT: &[u8] = include_bytes!("../browser/index.html");
    Html(ROOT)
}

async fn css_page() -> impl IntoResponse {
    const F: &[u8] = include_bytes!("../browser/browser.css");
    ([(axum::http::header::CONTENT_TYPE, "text/css")], F)
}

async fn js_page() -> impl IntoResponse {
    const F: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/bundle.js"));
    ([(axum::http::header::CONTENT_TYPE, "text/javascript")], F)
}

#[resource]
impl Resource for HttpServer {
    fn new(
        _: comprehensive::NoDependencies,
        _: comprehensive::NoArgs,
        _: &mut AssemblyRuntime<'_>,
    ) -> Result<Arc<Self>, std::convert::Infallible> {
        let app = Router::new()
            .route("/", axum::routing::get(root_page))
            .route("/browser.css", axum::routing::get(css_page))
            .route("/browser.js", axum::routing::get(js_page));
        let builder = Builder::new(TokioExecutor::new());
        Ok(Arc::new(Self { builder, app }))
    }
}

impl HttpServer {
    pub async fn serve<T>(&self, io: T) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    where
        T: AsyncRead + AsyncWrite + Unpin + 'static,
    {
        self.builder
            .serve_connection(TokioIo::new(io), TowerToHyperService::new(self.app.clone()))
            .await
    }
}
