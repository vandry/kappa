use axum::body::Body;
use bytes::Bytes;
use comprehensive::v1::{AssemblyRuntime, Resource, resource};
use futures::Stream;
use http::Response;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder;
use hyper_util::service::TowerToHyperService;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite};

pub struct ErrorServer {
    builder: Builder<TokioExecutor>,
}

#[resource]
impl Resource for ErrorServer {
    fn new(
        _: comprehensive::NoDependencies,
        _: comprehensive::NoArgs,
        _: &mut AssemblyRuntime<'_>,
    ) -> Result<Arc<Self>, std::convert::Infallible> {
        let builder = Builder::new(TokioExecutor::new());
        Ok(Arc::new(Self { builder }))
    }
}

pub trait ErrorHtml: Send + Sync + 'static {
    fn to_html(&self) -> String;
}

impl ErrorHtml for std::io::Error {
    fn to_html(&self) -> String {
        format!(
            r#"
            <p>
                IO error from session tunnel
                <ul>
                    <li>{}</li>
                </ul>
            </p>
        "#,
            html_escape::encode_text(&self.to_string())
        )
    }
}

impl ErrorHtml for tonic::Status {
    fn to_html(&self) -> String {
        format!(
            r#"
            <p>
                gRPC error from session tunnel
                <ul>
                    <li><b>Code:</b> {}</li>
                    <li><b>Message:</b> {}</li>
                </ul>
            </p>
        "#,
            self.code().description(),
            html_escape::encode_text(self.message())
        )
    }
}

enum ErrorBodyPart {
    Literal(Bytes),
    Error(Arc<dyn ErrorHtml>),
}

struct ErrorBody<const L: usize>(std::array::IntoIter<ErrorBodyPart, L>);

impl<const L: usize> Stream for ErrorBody<L> {
    type Item = Result<Bytes, std::convert::Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(match self.0.next() {
            Some(ErrorBodyPart::Literal(b)) => Some(Ok(b)),
            Some(ErrorBodyPart::Error(e)) => Some(Ok(e.to_html().into())),
            None => None,
        })
    }
}

fn error_body(err: Arc<dyn ErrorHtml>) -> ErrorBody<3> {
    ErrorBody(
        [
            ErrorBodyPart::Literal(Bytes::from_static(
                br##"
            <html><head><title>Kappa: Tunnel Failed</title></head><body>
            <h1>Kappa: Tunnel Failed</h1>
        "##,
            )),
            ErrorBodyPart::Error(err),
            ErrorBodyPart::Literal(Bytes::from_static(b"</body></html>")),
        ]
        .into_iter(),
    )
}

#[derive(Clone)]
struct ErrorService(Arc<dyn ErrorHtml>);

impl<T> tower_service::Service<T> for ErrorService {
    type Response = Response<Body>;
    type Error = http::Error;
    type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _: T) -> Self::Future {
        std::future::ready(
            Response::builder()
                .status(http::StatusCode::BAD_GATEWAY)
                .header(http::header::CONTENT_TYPE, mime::TEXT_HTML_UTF_8.as_ref())
                .body(Body::from_stream(error_body(Arc::clone(&self.0)))),
        )
    }
}

impl ErrorServer {
    pub async fn serve<T>(
        &self,
        io: T,
        err: Arc<dyn ErrorHtml>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    where
        T: AsyncRead + AsyncWrite + Unpin + 'static,
    {
        self.builder
            .serve_connection(
                TokioIo::new(io),
                TowerToHyperService::new(ErrorService(err)),
            )
            .await
    }
}
