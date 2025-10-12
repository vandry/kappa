use backoff::ExponentialBackoffBuilder;
use comprehensive::health::HealthReporter;
use comprehensive::v1::{AssemblyRuntime, Resource, resource};
use futures::future::TryFutureExt as _;
use k8s_openapi::api::core::v1::Pod;
use kube::{Api, Client};
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::SetOnce;
use tonic::Status;

pub struct KubeApi(SetOnce<Client>);

#[resource]
impl Resource for KubeApi {
    fn new(
        (health_reporter,): (Arc<HealthReporter>,),
        _: comprehensive::NoArgs,
        api: &mut AssemblyRuntime<'_>,
    ) -> Result<Arc<Self>, comprehensive::ComprehensiveError> {
        let shared = Arc::new(Self(SetOnce::new()));
        let setter = shared.clone();
        let signaller = health_reporter.register("KubeApi")?;
        let backoff = ExponentialBackoffBuilder::new()
            .with_max_elapsed_time(None) // Never completely give up.
            .build();
        api.set_task(async move {
            let client = backoff::future::retry_notify(
                backoff,
                || Client::try_default().map_err(backoff::Error::transient),
                |e, _| {
                    log::warn!("Creating kube::Client: {e}");
                },
            )
            .await?;
            if setter.0.set(client).is_ok() {
                signaller.set_healthy(true);
            }
            Ok(())
        });
        Ok(shared)
    }
}

#[derive(Debug, Error)]
pub enum KubeApiError {
    #[error("{0}")]
    Kube(#[from] kube::Error),
    #[error("{0}")]
    DeadlineExceeded(#[from] tokio::time::error::Elapsed),
    #[error("Missing properties on k8s_openapi object")]
    MissingInformation,
    #[error("Error parsing IP address on k8s_openapi object: {0}")]
    AddrParseError(#[from] std::net::AddrParseError),
}

impl From<KubeApiError> for Status {
    fn from(e: KubeApiError) -> Status {
        match e {
            KubeApiError::Kube(ref ke) => match ke {
                kube::Error::Api(er) => match er.code {
                    403 => Status::permission_denied(e.to_string()),
                    404 => Status::not_found(e.to_string()),
                    _ => Status::internal(e.to_string()),
                },
                _ => Status::internal(e.to_string()),
            },
            KubeApiError::DeadlineExceeded(e) => Status::deadline_exceeded(e.to_string()),
            KubeApiError::MissingInformation => Status::unavailable(e.to_string()),
            KubeApiError::AddrParseError(e) => Status::internal(e.to_string()),
        }
    }
}

const TIMEOUT: Duration = Duration::new(20, 0);

impl KubeApi {
    #[allow(dead_code)]
    pub async fn get_pod_ip(
        &self,
        namespace: &str,
        pod_name: &str,
    ) -> Result<IpAddr, KubeApiError> {
        tokio::time::timeout(TIMEOUT, async move {
            let api: Api<Pod> = Api::namespaced(self.0.wait().await.clone(), namespace);
            Ok(api
                .get(pod_name)
                .await?
                .status
                .ok_or(KubeApiError::MissingInformation)?
                .pod_ip
                .ok_or(KubeApiError::MissingInformation)?
                .parse()?)
        })
        .await?
    }

    #[allow(dead_code)]
    pub async fn list_pods(&self) -> Result<Vec<Pod>, KubeApiError> {
        tokio::time::timeout(TIMEOUT, async move {
            let api: Api<Pod> = Api::all(self.0.wait().await.clone());
            Ok(api.list(&kube::api::ListParams::default()).await?.items)
        })
        .await?
    }
}
