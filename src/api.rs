use comprehensive::v1::{AssemblyRuntime, Resource, resource};
use comprehensive::{NoArgs, ResourceDependencies};
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
struct Api {
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

#[derive(ResourceDependencies)]
pub struct ApiDependencies {
    kube: Arc<crate::kube::KubeApi>,
}

#[resource]
#[export_grpc(KappaBrowserServer)]
#[proto_descriptor(crate::pb::FILE_DESCRIPTOR_SET)]
impl Resource for Api {
    fn new(
        d: ApiDependencies,
        _: NoArgs,
        _: &mut AssemblyRuntime<'_>,
    ) -> Result<Arc<Self>, std::convert::Infallible> {
        Ok(Arc::new(Self { kube: d.kube }))
    }
}

pub struct ApiServer {
    server: tonic::transport::Server<Stack<GrpcWebLayer, Stack<CorsLayer, tower_layer::Identity>>>,
    routes: Arc<comprehensive_grpc::server::GrpcServingRoutes>,
}

#[derive(ResourceDependencies)]
pub struct ApiServerDependencies {
    routes: Arc<comprehensive_grpc::server::GrpcServingRoutes>,
    _service: PhantomData<Api>,
}

#[resource]
impl Resource for ApiServer {
    fn new(
        d: ApiServerDependencies,
        _: NoArgs,
        _: &mut AssemblyRuntime<'_>,
    ) -> Result<Arc<Self>, std::convert::Infallible> {
        let server = tonic::transport::Server::builder()
            .accept_http1(true)
            .layer(CorsLayer::permissive())
            .layer(GrpcWebLayer::new());
        Ok(Arc::new(Self {
            server,
            routes: d.routes,
        }))
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
            .add_routes(self.routes.routes())
            .serve_with_incoming(stream)
            .await
    }
}
