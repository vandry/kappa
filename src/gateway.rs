use async_stream::stream;
use comprehensive::v1::{AssemblyRuntime, Resource, resource};
use comprehensive::{NoArgs, ResourceDependencies};
use futures::future::Either;
use futures::pin_mut;
use futures::{Stream, StreamExt};
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpSocket;
use tokio::task::JoinHandle;
use tonic::Status;

use crate::pb::{StreamRequest, StreamResponse};

pub struct Gateway {
    acl: Arc<crate::acl::SimpleACL>,
    kube: Arc<crate::kube::KubeApi>,
}

#[derive(ResourceDependencies)]
pub struct GatewayDependencies {
    acl: Arc<crate::acl::SimpleACL>,
    kube: Arc<crate::kube::KubeApi>,
}

#[resource]
#[export_grpc(crate::pb::gateway_server::GatewayServer)]
#[proto_descriptor(crate::pb::FILE_DESCRIPTOR_SET)]
impl Resource for Gateway {
    fn new(
        d: GatewayDependencies,
        _: NoArgs,
        _: &mut AssemblyRuntime<'_>,
    ) -> Result<Arc<Self>, std::convert::Infallible> {
        Ok(Arc::new(Self {
            acl: d.acl,
            kube: d.kube,
        }))
    }
}

fn read_from_backend<I>(
    mut backend: I,
    task: JoinHandle<Result<(), Status>>,
) -> impl Stream<Item = Result<StreamResponse, Status>>
where
    I: AsyncRead + Unpin,
{
    stream! {
        let mut buf = [0; 4096];
        pin_mut!(task);
        loop {
            let reader = backend.read(&mut buf);
            pin_mut!(reader);
            match futures::future::select(task, reader).await {
                Either::Left((Err(e), _)) => {
                    yield Err(Status::internal(e.to_string()));  // join error
                    break;
                }
                Either::Left((Ok(Err(e)), _)) => {
                    yield Err(e);  // error from tx.write_all
                    break;
                }
                Either::Left((Ok(Ok(())), _)) => {
                    break;  // in stream finished
                }
                Either::Right((Err(e), _)) => {
                    yield Err(e.into());  // error from rx.read
                    break;
                }
                Either::Right((Ok(0), _)) => {
                    break;  // eof from rx.read
                }
                Either::Right((Ok(size), ret_task)) => {
                    task = ret_task;
                    yield Ok(StreamResponse{b: Some(buf[..size].to_vec())});
                }
            }
        }
    }
}

async fn write_to_backend<I, O>(
    mut b: Option<Vec<u8>>,
    mut in_stream: I,
    mut tx: O,
) -> Result<(), Status>
where
    I: Stream<Item = Result<StreamRequest, Status>> + Unpin,
    O: AsyncWrite + Unpin,
{
    loop {
        if let Some(v) = b {
            if !v.is_empty() {
                tx.write_all(&v).await?;
            }
        }
        b = match in_stream.next().await {
            Some(frame) => frame?.b,
            None => {
                break Ok(());
            }
        };
    }
}

#[tonic::async_trait]
impl crate::pb::gateway_server::Gateway for Gateway {
    type StreamStream = Pin<Box<dyn Stream<Item = Result<StreamResponse, Status>> + Send>>;

    async fn stream(
        &self,
        req: tonic::Request<tonic::Streaming<StreamRequest>>,
    ) -> Result<tonic::Response<Self::StreamStream>, Status> {
        let peer = self.acl.allowed(req.peer_certs())?;
        let mut in_stream = req.into_inner();
        let frame = match in_stream.next().await {
            Some(frame) => Ok(frame?),
            None => Err(Status::out_of_range("Empty stream, no connection possible")),
        }?;
        let dest = frame.connect_to.ok_or(Status::invalid_argument(
            "First frame of stream must have connect_to",
        ))?;
        let addr = self
            .kube
            .get_pod_ip(
                dest.namespace.as_deref().unwrap_or_default(),
                dest.pod.as_deref().unwrap_or_default(),
            )
            .await?;
        let port: u16 = dest
            .port
            .unwrap_or(65536)
            .try_into()
            .map_err(|_| Status::invalid_argument("invalid port number"))?;
        let sa: SocketAddr = SocketAddr::from((addr, port));
        log::info!("Client {peer} connected for {dest:?} resolved to {sa}");
        let sock = match sa {
            SocketAddr::V4(_) => TcpSocket::new_v4(),
            SocketAddr::V6(_) => TcpSocket::new_v6(),
        }?;
        let (rx, tx) = sock.connect(sa).await?.into_split();
        let task = tokio::spawn(async move { write_to_backend(frame.b, in_stream, tx).await });
        Ok(tonic::Response::new(Box::pin(read_from_backend(rx, task))))
    }
}
