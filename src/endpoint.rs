use std::sync::Arc;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncWrite, copy_bidirectional};
use tonic::transport::server::Connected;

use crate::error_server::ErrorServer;

crate::multiio::multiio! {
    MultiIO {
        E(crate::encap::EncapIO),
        T(tokio_rustls::client::TlsStream<crate::encap::EncapIO>),
    }
}

pub enum ConnectedEndpoint {
    Copy(MultiIO),
    Error(Arc<ErrorServer>, Arc<dyn crate::error_server::ErrorHtml>),
    Http(Arc<crate::http_server::HttpServer>),
    Api(Arc<crate::api::ApiServer>),
}

#[derive(Debug, Error)]
pub enum ServeError {
    #[error("{0}")]
    IOError(#[from] std::io::Error),
    #[error("{0}")]
    Http(String),
}

impl ConnectedEndpoint {
    pub async fn serve<T>(self, mut io: T) -> Result<(), ServeError>
    where
        T: Connected + AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        match self {
            Self::Copy(mut other) => {
                copy_bidirectional(&mut other, &mut io).await?;
                Ok(())
            }
            Self::Http(http_server) => http_server
                .serve(io)
                .await
                .map_err(|e| ServeError::Http(e.to_string())),
            Self::Api(http_server) => http_server
                .serve(io)
                .await
                .map_err(|e| ServeError::Http(e.to_string())),
            Self::Error(http_server, err) => http_server
                .serve(io, err)
                .await
                .map_err(|e| ServeError::Http(e.to_string())),
        }
    }
}

pub enum Endpoint {
    Encap(crate::encap::Connecting),
    Tls(crate::mtls::ConnectInSequence<crate::encap::Connecting>),
    Http(Arc<crate::http_server::HttpServer>),
    Api(Arc<crate::api::ApiServer>),
}

pub trait Connectable: Sized {
    type IO: AsyncRead + AsyncWrite + Unpin;

    async fn connect(self) -> Result<Self::IO, Arc<dyn crate::error_server::ErrorHtml>>;

    async fn connect_or_serve_error(self, error_server: Arc<ErrorServer>) -> ConnectedEndpoint
    where
        Self::IO: Into<MultiIO>,
    {
        match self.connect().await {
            Ok(io) => ConnectedEndpoint::Copy(io.into()),
            Err(e) => ConnectedEndpoint::Error(error_server, e),
        }
    }
}

impl Endpoint {
    pub async fn connect(self, error_server: Arc<ErrorServer>) -> ConnectedEndpoint {
        match self {
            Self::Encap(connecting) => connecting.connect_or_serve_error(error_server).await,
            Self::Tls(connecting) => connecting.connect_or_serve_error(error_server).await,
            Self::Http(http_server) => ConnectedEndpoint::Http(http_server),
            Self::Api(http_server) => ConnectedEndpoint::Api(http_server),
        }
    }
}
