use comprehensive::v1::{AssemblyRuntime, Resource, resource};
use comprehensive_tls::TlsConfig;
use http::Uri;
use std::sync::Arc;
use thiserror::Error;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls::client::ClientConfig;
use tokio_rustls::rustls::pki_types::ServerName;

use crate::endpoint::Connectable;
use crate::error_server::ErrorHtml;

#[derive(Debug, Error)]
pub enum AddMtlsError {
    #[error("{0}")]
    Comprehensive(#[from] comprehensive_tls::ComprehensiveTlsError),
    #[error("{0}")]
    ServerName(#[from] tokio_rustls::rustls::pki_types::InvalidDnsNameError),
}

pub struct AddMtls(Arc<TlsConfig>);

#[resource]
impl Resource for AddMtls {
    fn new(
        (tls_config,): (Arc<TlsConfig>,),
        _: comprehensive::NoArgs,
        _: &mut AssemblyRuntime<'_>,
    ) -> Result<Arc<Self>, std::convert::Infallible> {
        Ok(Arc::new(Self(tls_config)))
    }
}

pub struct ConnectInSequence<T> {
    inner: T,
    tls_config: Arc<ClientConfig>,
    name: ServerName<'static>,
}

impl<T: Connectable> Connectable for ConnectInSequence<T> {
    type IO = TlsStream<T::IO>;

    async fn connect(self) -> Result<Self::IO, Arc<dyn ErrorHtml>> {
        let inner = self.inner.connect().await?;
        let connector = TlsConnector::from(self.tls_config);
        match connector.connect(self.name, inner).await {
            Ok(io) => Ok(io),
            Err(e) => Err(Arc::new(e)),
        }
    }
}

impl AddMtls {
    pub fn connect<T>(
        &self,
        server_identity: &Uri,
        inner: T,
    ) -> Result<ConnectInSequence<T>, AddMtlsError>
    where
        T: Connectable,
    {
        let mut c = self.0.client_config(server_identity, None)?;
        c.enable_sni = false; // We have no sensible host name to use.
        c.alpn_protocols = vec![b"h2".to_vec()];
        let name = if server_identity.scheme() == Some(&http::uri::Scheme::HTTPS) {
            ServerName::try_from(server_identity.host().unwrap_or_default())?
        } else {
            ServerName::try_from("_").unwrap()
        };
        Ok(ConnectInSequence {
            inner,
            tls_config: Arc::new(c),
            name: name.to_owned(),
        })
    }
}
