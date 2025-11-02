use comprehensive::NoArgs;
use comprehensive::v1::{AssemblyRuntime, Resource, resource};
use itertools::Itertools;
use std::marker::PhantomData;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};
use tonic::Status;
use tonic::transport::server::Connected;
use tonic_web::GrpcWebLayer;
use tower_http::cors::CorsLayer;
use tower_layer::Stack;

use crate::pb::kappa_browser_server::{KappaBrowser, KappaBrowserServer};

#[derive(Clone)]
pub struct Api {
    kube: Arc<crate::kube::KubeApi>,
}

#[tonic::async_trait]
impl KappaBrowser for Api {
    async fn list_pods(
        &self,
        _: tonic::Request<crate::pb::ListPodsRequest>,
    ) -> Result<tonic::Response<crate::pb::ListPodsResponse>, Status> {
        Ok(tonic::Response::new(crate::pb::ListPodsResponse {
            pods: self
                .kube
                .list_pods()
                .await?
                .into_iter()
                .map_into()
                .collect(),
        }))
    }
}

#[resource]
#[export_grpc(KappaBrowserServer)]
#[proto_descriptor(crate::pb::FILE_DESCRIPTOR_SET)]
impl Resource for Api {
    fn new(
        (kube,): (Arc<crate::kube::KubeApi>,),
        _: NoArgs,
        _: &mut AssemblyRuntime<'_>,
    ) -> Result<Arc<Self>, std::convert::Infallible> {
        Ok(Arc::new(Self { kube }))
    }
}

pub struct ApiServer {
    server: tonic::transport::Server<Stack<GrpcWebLayer, Stack<CorsLayer, tower_layer::Identity>>>,
    service: Arc<comprehensive_grpc::server::GrpcCommonService>,
}

#[resource]
impl Resource for ApiServer {
    fn new(
        (service, _): (
            Arc<comprehensive_grpc::server::GrpcCommonService>,
            PhantomData<Api>,
        ),
        _: NoArgs,
        _: &mut AssemblyRuntime<'_>,
    ) -> Result<Arc<Self>, std::convert::Infallible> {
        let server = tonic::transport::Server::builder()
            .accept_http1(true)
            .layer(CorsLayer::permissive())
            .layer(GrpcWebLayer::new());
        Ok(Arc::new(Self { server, service }))
    }
}

impl ApiServer {
    pub async fn serve<T>(&self, io: T) -> Result<(), tonic::transport::Error>
    where
        T: Connected + AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let stream =
            futures::stream::once(std::future::ready(Ok::<_, std::convert::Infallible>(io)));
        self.server
            .clone()
            .serve_with_incoming((*self.service).clone().into_service(), stream)
            .await
    }
}
