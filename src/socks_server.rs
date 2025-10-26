use comprehensive::v1::{AssemblyRuntime, Resource, TaskWithCleanup, resource};
use futures::future::Either;
use std::os::fd::AsRawFd as _;
use std::path::PathBuf;
use std::pin::pin;
use std::sync::Arc;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::UnixListener;
use tokio_util::sync::CancellationToken;
use tonic::transport::server::Connected;
use tracing::instrument::Instrument as _;
use tracing::{Level, Span, span};

use crate::error_server::ErrorServer;
use crate::router::DomainRouter;
use crate::socks_proto;

use socks_proto::send_reply;

#[derive(Debug, Error)]
enum SocksServingError {
    #[error("{0}")]
    IOError(#[from] std::io::Error),
    #[error("{0}")]
    Socks(#[from] socks_proto::SocksProtocolError),
    #[error("{0}")]
    Serving(#[from] crate::endpoint::ServeError),
}

async fn serve_socks<T>(
    mut s: T,
    router: Arc<DomainRouter>,
    error_server: Arc<ErrorServer>,
) -> Result<(), SocksServingError>
where
    T: Connected + AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let methods = socks_proto::read_client_hello(&mut s).await?;
    if !methods.contains(&socks_proto::NO_AUTHENTICATION) {
        socks_proto::server_hello_no_acceptable_auth(&mut s).await?;
        return Ok(());
    }
    socks_proto::server_hello_no_auth(&mut s).await?;
    let request = socks_proto::read_socks_request(&mut s).await?;
    let socks_proto::SocksRequest::Connect(addr, port) = request else {
        send_reply(&mut s, socks_proto::REP_COMMAND_NOT_SUPPORTED).await?;
        return Ok(());
    };
    let socks_proto::SocksAddr::Domain(domain) = addr else {
        send_reply(&mut s, socks_proto::REP_ADDRESS_TYPE_NOT_SUPPORTED).await?;
        return Ok(());
    };
    let endpoint = router.connect(&domain, port).map_err(|e| {
        log::info!("Failed to connect to {domain:?}:{port}: {e}");
        e.into()
    });
    match endpoint {
        Ok(ep) => {
            send_reply(&mut s, socks_proto::REP_SUCCEEDED).await?;
            log::info!("CONNECT {domain:?}:{}", port);
            ep.connect(error_server).await.serve(s).await?;
        }
        Err(e) => {
            send_reply(&mut s, e).await?;
        }
    }
    Ok(())
}

pub struct SocksServer;

#[derive(clap::Args)]
pub struct SocksServerArgs {
    #[arg(
        long,
        help = "Path of UNIX socket to listen on (browser should connect to this)"
    )]
    socks_listen: PathBuf,
}

struct SocksServerTask {
    cancel: CancellationToken,
    listener_and_router: Option<(UnixListener, Arc<DomainRouter>, Arc<ErrorServer>)>,
    socket_path: PathBuf,
}

impl TaskWithCleanup for SocksServerTask {
    #[allow(refining_impl_trait)]
    async fn main_task(&mut self) -> Result<(), std::convert::Infallible> {
        let (listener, router, error_server) = self.listener_and_router.take().unwrap();
        let cancel = self.cancel.clone();
        let _ = tokio::spawn(
            async move {
                let mut cancel_fut = pin!(cancel.cancelled());
                loop {
                    match futures::future::select(pin!(listener.accept()), &mut cancel_fut).await {
                        Either::Right(((), _)) => {
                            break;
                        }
                        Either::Left((Ok((s, _)), _)) => {
                            let cancel2 = cancel.clone();
                            let router2 = router.clone();
                            let error_server2 = error_server.clone();
                            let tracing_span =
                                span!(Level::INFO, "connection", fd = s.as_raw_fd()).or_current();
                            tokio::spawn(
                                async move {
                                    cancel2
                                        .run_until_cancelled_owned(async move {
                                            if let Err(e) =
                                                serve_socks(s, router2, error_server2).await
                                            {
                                                log::warn!("Socks connection: {e}");
                                            }
                                        })
                                        .await
                                }
                                .instrument(tracing_span),
                            );
                        }
                        Either::Left((Err(e), _)) => {
                            log::error!("UNIX socket accept error: {e}");
                        }
                    }
                }
            }
            .instrument(Span::current()),
        )
        .await;
        Ok(())
    }

    #[allow(refining_impl_trait)]
    async fn cleanup(self) -> Result<(), std::io::Error> {
        self.cancel.cancel();
        std::fs::remove_file(self.socket_path)
    }
}

#[resource]
impl Resource for SocksServer {
    fn new(
        (router, error_server): (Arc<DomainRouter>, Arc<ErrorServer>),
        a: SocksServerArgs,
        api: &mut AssemblyRuntime<'_>,
    ) -> Result<Arc<Self>, std::io::Error> {
        disk::umask(0o077);
        let listener = UnixListener::bind(&a.socks_listen).or_else(|e| match e.kind() {
            std::io::ErrorKind::AddrInUse => {
                match std::os::unix::net::UnixStream::connect(&a.socks_listen) {
                    Ok(_) => Err(e),
                    Err(e) => match e.kind() {
                        std::io::ErrorKind::ConnectionRefused => {
                            match std::fs::remove_file(&a.socks_listen) {
                                Ok(()) => UnixListener::bind(&a.socks_listen),
                                Err(_) => Err(e),
                            }
                        }
                        _ => Err(e),
                    },
                }
            }
            _ => Err(e),
        })?;
        api.set_task_with_cleanup(SocksServerTask {
            cancel: CancellationToken::new(),
            listener_and_router: Some((listener, router, error_server)),
            socket_path: a.socks_listen,
        });
        Ok(Arc::new(Self))
    }
}
