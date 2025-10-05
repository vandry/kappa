use comprehensive::ResourceDependencies;
use comprehensive::v1::{AssemblyRuntime, Resource, resource};
use comprehensive_tls::TlsConfig;
use http::Uri;
use pin_project_lite::pin_project;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker, ready};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::{Connect, TlsConnector};

#[derive(Debug, Error)]
pub enum AddMtlsError {
    #[error("{0}")]
    Comprehensive(#[from] comprehensive_tls::ComprehensiveTlsError),
    #[error("{0}")]
    ServerName(#[from] tokio_rustls::rustls::pki_types::InvalidDnsNameError),
}

pub struct AddMtls(Arc<TlsConfig>);

#[derive(ResourceDependencies)]
pub struct AddMtlsDependencies(Arc<TlsConfig>);

#[resource]
impl Resource for AddMtls {
    fn new(
        d: AddMtlsDependencies,
        _: comprehensive::NoArgs,
        _: &mut AssemblyRuntime<'_>,
    ) -> Result<Arc<Self>, std::convert::Infallible> {
        Ok(Arc::new(Self(d.0)))
    }
}

pin_project! {
    struct Connecting<T> {
        #[pin] inner: Connect<T>,
        write_waker: Option<Waker>,
    }
}

impl<T> From<Connect<T>> for Connecting<T> {
    fn from(inner: Connect<T>) -> Self {
        Self {
            inner,
            write_waker: None,
        }
    }
}

impl<T> Connecting<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<TlsStream<T>, std::io::Error>> {
        let r = ready!(self.as_mut().project().inner.poll(cx));
        if let Some(waker) = self.write_waker.take() {
            waker.wake();
        }
        Poll::Ready(r)
    }

    fn poll_write<R>(&mut self, cx: &mut Context<'_>) -> Poll<R> {
        let my_waker = cx.waker();
        if let Some(ref waker) = self.write_waker {
            if waker.will_wake(my_waker) {
                return Poll::Pending;
            }
        }
        self.write_waker = Some(my_waker.clone());
        Poll::Pending
    }
}

pin_project! {
    #[project = AddMtlsIOProj]
    pub enum AddMtlsIO<T> {
        InProgress { #[pin] c: Connecting<T> },
        Connected { #[pin] inner: TlsStream<T> },
        Broken,
    }
}

impl<T> AddMtlsIO<T> {
    fn new(inner: Connect<T>) -> Self {
        Self::InProgress { c: inner.into() }
    }
}

impl<T> AsyncRead for AddMtlsIO<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.as_mut().project() {
            AddMtlsIOProj::InProgress { c } => match ready!(c.poll(cx)) {
                Ok(s) => {
                    self.set(Self::Connected { inner: s });
                    self.poll_read(cx, buf)
                }
                Err(e) => {
                    self.set(Self::Broken);
                    Poll::Ready(Err(e))
                }
            },
            AddMtlsIOProj::Broken => Poll::Ready(Err(std::io::ErrorKind::Other.into())),
            AddMtlsIOProj::Connected { inner } => inner.poll_read(cx, buf),
        }
    }
}

impl<T> AsyncWrite for AddMtlsIO<T>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        match self.as_mut().project() {
            AddMtlsIOProj::InProgress { mut c } => c.poll_write(cx),
            AddMtlsIOProj::Broken => Poll::Ready(Err(std::io::ErrorKind::Other.into())),
            AddMtlsIOProj::Connected { inner } => inner.poll_write(cx, buf),
        }
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        match self.as_mut().project() {
            AddMtlsIOProj::InProgress { mut c } => c.poll_write(cx),
            AddMtlsIOProj::Broken => Poll::Ready(Err(std::io::ErrorKind::Other.into())),
            AddMtlsIOProj::Connected { inner } => inner.poll_shutdown(cx),
        }
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        match self.as_mut().project() {
            AddMtlsIOProj::InProgress { mut c } => c.poll_write(cx),
            AddMtlsIOProj::Broken => Poll::Ready(Err(std::io::ErrorKind::Other.into())),
            AddMtlsIOProj::Connected { inner } => inner.poll_flush(cx),
        }
    }
}

impl AddMtls {
    pub fn connect<T>(&self, server_identity: &Uri, inner: T) -> Result<AddMtlsIO<T>, AddMtlsError>
    where
        T: AsyncRead + AsyncWrite + Unpin,
    {
        let mut c = self.0.client_config(server_identity, None)?;
        c.enable_sni = false; // We have no sensible host name to use.
        c.alpn_protocols = vec![b"h2".to_vec()];
        let name = if server_identity.scheme() == Some(&http::uri::Scheme::HTTPS) {
            ServerName::try_from(server_identity.host().unwrap_or_default())?
        } else {
            ServerName::try_from("_").unwrap()
        };
        Ok(AddMtlsIO::new(
            TlsConnector::from(Arc::new(c)).connect(name.to_owned(), inner),
        ))
    }
}
