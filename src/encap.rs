use async_stream::stream;
use comprehensive::ResourceDependencies;
use comprehensive::v1::{AssemblyRuntime, Resource, resource};
use comprehensive_grpc::GrpcClient;
use futures::{Stream, StreamExt};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker, ready};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tonic::{Code, Status};

use crate::pb::{Destination, StreamRequest, StreamResponse};

#[derive(GrpcClient)]
struct GatewayClient(crate::pb::gateway_client::GatewayClient<comprehensive_grpc::client::Channel>);

pub struct GatewayEncap(Arc<GatewayClient>);

#[derive(ResourceDependencies)]
pub struct GatewayEncapDependencies(Arc<GatewayClient>);

#[resource]
impl Resource for GatewayEncap {
    fn new(
        d: GatewayEncapDependencies,
        _: comprehensive::NoArgs,
        _: &mut AssemblyRuntime<'_>,
    ) -> Result<Arc<Self>, std::convert::Infallible> {
        Ok(Arc::new(Self(d.0)))
    }
}

#[derive(Default)]
struct ToGateway {
    len: usize,
    data: Vec<Vec<u8>>,
    eof: bool,
    waiting_for_data: Option<Waker>,
    waiting_for_reader: Option<Waker>,
}

const TO_GATEWAY_BUFFER: usize = 500;

impl ToGateway {
    fn poll_write(
        &mut self,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        if self.len >= TO_GATEWAY_BUFFER {
            if let Err(e) = ready!(self.poll_flush(cx)) {
                return Poll::Ready(Err(e));
            }
        }
        self.data.push(buf.to_vec());
        self.len += buf.len();
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_shutdown(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        self.eof = true;
        self.poll_flush(cx)
    }

    fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        if self.data.is_empty() {
            Poll::Ready(Ok(()))
        } else {
            if let Some(waker) = self.waiting_for_data.take() {
                waker.wake();
            }
            self.waiting_for_reader = Some(cx.waker().clone());
            Poll::Pending
        }
    }

    fn poll_next(&mut self, cx: &mut Context<'_>) -> Poll<Option<Vec<Vec<u8>>>> {
        if !self.data.is_empty() {
            let data = std::mem::take(&mut self.data);
            self.len = 0;
            return Poll::Ready(Some(data));
        }
        if self.eof {
            return Poll::Ready(None);
        }
        if let Some(waker) = self.waiting_for_reader.take() {
            waker.wake();
        }
        self.waiting_for_data = Some(cx.waker().clone());
        Poll::Pending
    }
}

struct UnfinishedMeal {
    food: Vec<u8>,
    eaten: usize,
}

pub struct EncapIO {
    to_gateway: Arc<Mutex<ToGateway>>,
    from_gateway: Pin<Box<dyn Stream<Item = Result<StreamResponse, Status>> + Send>>,
    unfinished_meal: Option<UnfinishedMeal>,
}

impl EncapIO {
    fn new(
        to_gateway: Arc<Mutex<ToGateway>>,
        from_gateway: Pin<Box<dyn Stream<Item = Result<StreamResponse, Status>> + Send>>,
    ) -> Self {
        Self {
            to_gateway,
            from_gateway,
            unfinished_meal: None,
        }
    }
}

fn status_to_io(e: Status) -> std::io::Error {
    let kind = match e.code() {
        Code::InvalidArgument => std::io::ErrorKind::InvalidInput,
        Code::DeadlineExceeded => std::io::ErrorKind::TimedOut,
        Code::NotFound => std::io::ErrorKind::NotFound,
        Code::AlreadyExists => std::io::ErrorKind::AlreadyExists,
        Code::PermissionDenied => std::io::ErrorKind::PermissionDenied,
        Code::ResourceExhausted => std::io::ErrorKind::QuotaExceeded,
        Code::FailedPrecondition => std::io::ErrorKind::InvalidInput,
        Code::Unauthenticated => std::io::ErrorKind::PermissionDenied,
        _ => std::io::ErrorKind::Other,
    };
    std::io::Error::new(kind, e)
}

impl AsyncRead for EncapIO {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let room = buf.remaining();
        if let Some(ref mut meal) = self.unfinished_meal {
            let food = &meal.food[meal.eaten..];
            if food.len() <= room {
                buf.put_slice(food);
                self.unfinished_meal = None;
            } else if room > 0 {
                buf.put_slice(&food[..room]);
                meal.eaten += room;
            }
            return Poll::Ready(Ok(()));
        }
        loop {
            break match ready!(self.from_gateway.as_mut().poll_next(cx)) {
                Some(Ok(frame)) => {
                    let food = frame.b.unwrap_or_default();
                    if food.is_empty() {
                        continue;
                    }
                    if food.len() <= room {
                        buf.put_slice(&food);
                    } else {
                        if room > 0 {
                            buf.put_slice(&food[..room]);
                        }
                        self.unfinished_meal = Some(UnfinishedMeal { food, eaten: room });
                    }
                    Poll::Ready(Ok(()))
                }
                Some(Err(e)) => Poll::Ready(Err(status_to_io(e))),
                None => Poll::Ready(Ok(())),
            };
        }
    }
}

impl AsyncWrite for EncapIO {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }
        self.to_gateway.lock().unwrap().poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        self.to_gateway.lock().unwrap().poll_flush(cx)
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        self.to_gateway.lock().unwrap().poll_shutdown(cx)
    }
}

struct ToGatewayStream {
    to_gateway: Arc<Mutex<ToGateway>>,
    connect_to: Option<Destination>,
}

impl ToGatewayStream {
    fn new(connect_to: Destination, to_gateway: Arc<Mutex<ToGateway>>) -> Self {
        Self {
            connect_to: Some(connect_to),
            to_gateway,
        }
    }
}

impl Stream for ToGatewayStream {
    type Item = StreamRequest;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let maybe_bufs = ready!(self.to_gateway.lock().unwrap().poll_next(cx));
        Poll::Ready(maybe_bufs.map(|bufs| StreamRequest {
            connect_to: self.connect_to.take(),
            b: Some(itertools::concat(bufs)),
        }))
    }
}

fn make_rpc(
    client: Arc<GatewayClient>,
    connect_to: Destination,
    to_gateway: Arc<Mutex<ToGateway>>,
) -> impl Stream<Item = Result<StreamResponse, Status>> {
    stream! {
        let mut s = match client.client().stream(ToGatewayStream::new(connect_to, to_gateway)).await {
            Err(e) => {
                yield Err(e);
                return;
            }
            Ok(s) => s.into_inner(),
        };
        while let Some(item) = s.next().await {
            yield item;
        }
    }
}

impl GatewayEncap {
    pub fn encap(
        &self,
        namespace: impl Into<String>,
        pod: impl Into<String>,
        port: u16,
    ) -> EncapIO {
        let to_gateway = Arc::new(Mutex::new(ToGateway::default()));
        let dest = Destination {
            namespace: Some(namespace.into()),
            pod: Some(pod.into()),
            port: Some(port.into()),
        };
        let from_gateway = Box::pin(make_rpc(self.0.clone(), dest, to_gateway.clone()));
        EncapIO::new(to_gateway, from_gateway)
    }
}
