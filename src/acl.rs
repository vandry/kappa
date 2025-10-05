use comprehensive::v1::{AssemblyRuntime, Resource, resource};
use http::Uri;
use itertools::Itertools as _;
use std::collections::HashSet;
use std::sync::Arc;
use tonic::transport::CertificateDer;
use tonic::{Code, Status};
use x509_parser::certificate::X509Certificate;
use x509_parser::prelude::FromDer;

pub struct SimpleACL {
    allowed: HashSet<Uri>,
}

#[derive(clap::Args)]
pub struct SimpleACLArgs {
    #[arg(long, help = "URI SAN allowed to connect")]
    acl: Vec<Uri>,
}

#[resource]
impl Resource for SimpleACL {
    fn new(
        _: comprehensive::NoDependencies,
        a: SimpleACLArgs,
        _: &mut AssemblyRuntime<'_>,
    ) -> Result<Arc<Self>, std::convert::Infallible> {
        Ok(Arc::new(Self {
            allowed: a.acl.into_iter().collect(),
        }))
    }
}

impl SimpleACL {
    pub fn allowed(
        &self,
        peer_certs: Option<Arc<Vec<CertificateDer<'static>>>>,
    ) -> Result<Uri, Status> {
        let certs = peer_certs
            .ok_or_else(|| Status::new(Code::PermissionDenied, "no client certificate"))?;
        let cert = certs
            .iter()
            .next()
            .ok_or_else(|| Status::new(Code::PermissionDenied, "no client certificate"))?;
        let x509 = X509Certificate::from_der(cert)
            .map_err(|e| {
                Status::new(
                    Code::PermissionDenied,
                    format!("error reading client certificate: {}", e),
                )
            })?
            .1;
        let san = x509
            .subject_alternative_name()
            .ok()
            .flatten()
            .and_then(|ext| ext.value.general_names.iter().exactly_one().ok())
            .and_then(|gn| match gn {
                x509_parser::extensions::GeneralName::URI(s) => Some(*s),
                _ => None,
            })
            .ok_or_else(|| Status::new(Code::PermissionDenied, "no URI SAN in certificate"))?
            .parse::<Uri>()
            .map_err(|_| Status::new(Code::PermissionDenied, "URI SAN not parsable"))?;
        if self.allowed.is_empty() || self.allowed.contains(&san) {
            Ok(san)
        } else {
            Err(Status::new(Code::PermissionDenied, "Client not allowed"))
        }
    }
}
