use comprehensive::v1::{AssemblyRuntime, Resource, resource};
use http::Uri;
use std::sync::Arc;
use thiserror::Error;

use crate::endpoint::Endpoint;

#[derive(Debug, Error)]
pub enum RouterError {
    #[error("{0}")]
    IOError(#[from] std::io::Error),
    #[error("{0}")]
    AddMtls(#[from] crate::mtls::AddMtlsError),
    #[error("Unrecognised suffix: got {0} expect {1}")]
    UnrecognisedSuffix(String, String),
    #[error("Expected (pod).suffix, got {0:?}")]
    BadSelector(Option<String>),
    #[error("Expected (http|https).(object).(namespace).(selector).suffix")]
    ExpectKubeObject,
    #[error("Hostname must be prefixed with escaped expected server identity")]
    BadServerIdentity,
    #[error("Expected browser.suffix or api.browser.suffix")]
    ExpectBrowser,
    #[error("Wrong port")]
    PortUnreachable,
    #[error("URI construction error")]
    UriError(#[from] http::Error),
}

pub struct DomainRouter {
    domain_suffix: Box<[String]>,
    encap: Arc<crate::encap::GatewayEncap>,
    add_mtls: Arc<crate::mtls::AddMtls>,
    http_server: Arc<crate::http_server::HttpServer>,
    api_server: Arc<crate::api::ApiServer>,
}

#[derive(clap::Args)]
pub struct DomainRouterArgs {
    #[arg(
        long,
        help = "Fixed suffix which must appear at the end of domain names and will be stripped"
    )]
    domain_suffix: Option<String>,
}

#[resource]
impl Resource for DomainRouter {
    fn new(
        (encap, add_mtls, http_server, api_server): (
            Arc<crate::encap::GatewayEncap>,
            Arc<crate::mtls::AddMtls>,
            Arc<crate::http_server::HttpServer>,
            Arc<crate::api::ApiServer>,
        ),
        a: DomainRouterArgs,
        _: &mut AssemblyRuntime<'_>,
    ) -> Result<Arc<Self>, std::io::Error> {
        Ok(Arc::new(Self {
            domain_suffix: a
                .domain_suffix
                .map(|s| s.split('.').map(ToString::to_string).collect())
                .unwrap_or_default(),
            encap,
            add_mtls,
            http_server,
            api_server,
        }))
    }
}

struct DecodeEscapedPath<I: Iterator> {
    delimiter: bool,
    inner: std::iter::Peekable<I>,
}

impl<I: Iterator> DecodeEscapedPath<I> {
    fn new(inner: I) -> Self {
        Self {
            delimiter: true,
            inner: inner.peekable(),
        }
    }
}

impl<'a, I> Iterator for DecodeEscapedPath<I>
where
    I: Iterator<Item = &'a str>,
{
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        if std::mem::take(&mut self.delimiter) {
            match self.inner.peek() {
                None => None,
                Some(&"_") => {
                    let _ = self.inner.next();
                    Some(".")
                }
                Some(_) => Some("/"),
            }
        } else {
            match self.inner.next() {
                None => None,
                Some(l) => {
                    self.delimiter = true;
                    Some(if let Some(ll) = l.strip_prefix("_") {
                        ll
                    } else {
                        l
                    })
                }
            }
        }
    }
}

/// The hostname given must encode the identity we expect to see from the
/// peer. Although it is a serious exercise in shoehorning, we use dots
/// in the hostname and the special sequence ._. to achieve the encoding.
///
/// The format is:
///
/// `scheme[.td-label]*[._[.path-label]*]?._
///
/// where
///   `scheme` is the URI scheme of the expected identity
///   `host-label` are joined with `.` to form the host
///   `path-label` are joined with `/` to form the URI path section
///     except that a label equal to `_` is deleted and causes the
///     surrounding labels to be joined with `.` instead and any other
///     label beginning with `_` is joined to its neighbours with `/` as
///     usual after deleting the `_` prefix.
/// `host-label` cannot be `_` because that signals the end of the host.
///
/// Examples:
///
/// spiffe.trust.domain._
///    => spiffe://trust.domain/
/// spiffe.trust.domain._.foo.bar._
///    => spiffe://trust.domain/foo/bar
/// spiffe.trust.domain._.contains.a._.dot._
///    => spiffe://trust.domain/contains/a.dot
/// spiffe.trust.domain._.contains.a.__.dot._
///    => spiffe://trust.domain/very/_/weird
fn extract_expected_identity<'a, I>(mut labels: I) -> Result<Uri, RouterError>
where
    I: DoubleEndedIterator + Iterator<Item = &'a str> + 'a,
{
    match labels.next_back() {
        Some("_") => (),
        _ => {
            return Err(RouterError::BadServerIdentity);
        }
    };
    let scheme = labels.next().ok_or(RouterError::BadServerIdentity)?;
    let host = join_string::join(labels.by_ref().take_while(|l| *l != "_"), ".");
    Ok(Uri::builder()
        .scheme(scheme)
        .authority(host.into_string())
        .path_and_query(DecodeEscapedPath::new(labels).collect::<String>())
        .build()?)
}

impl DomainRouter {
    pub fn connect(&self, name: &str, port: u16) -> Result<Endpoint, RouterError> {
        let mut labels = name.split('.');
        let mut want_suffix = self.domain_suffix.iter();
        while let Some(want_label) = want_suffix.next_back() {
            match labels.next_back() {
                Some(l) if l.eq_ignore_ascii_case(want_label) => (),
                _ => {
                    return Err(RouterError::UnrecognisedSuffix(
                        name.to_string(),
                        self.domain_suffix.join("."),
                    ));
                }
            }
        }
        let selector = labels
            .next_back()
            .ok_or_else(|| RouterError::BadSelector(None))?;
        if selector.eq_ignore_ascii_case("pod") {
            let namespace = labels.next_back().ok_or(RouterError::ExpectKubeObject)?;
            let pod = labels.next_back().ok_or(RouterError::ExpectKubeObject)?;
            let protocol = labels.next_back().ok_or(RouterError::ExpectKubeObject)?;
            let https_identity = match protocol {
                "http" => {
                    if labels.next().is_some() {
                        return Err(RouterError::ExpectKubeObject);
                    }
                    None
                }
                "https" => Some(extract_expected_identity(labels)?),
                _ => {
                    return Err(RouterError::ExpectKubeObject);
                }
            };
            log::info!(
                "Connect to gateway for namespace {namespace:?} pod {pod:?} port {port} server identity {https_identity:?}"
            );
            let encap = self.encap.encap(namespace, pod, port);
            return Ok(match https_identity {
                Some(uri) => Endpoint::Tls(self.add_mtls.connect(&uri, encap)?),
                None => Endpoint::Encap(encap),
            });
        } else if selector.eq_ignore_ascii_case("browser") {
            if port != 80 {
                return Err(RouterError::PortUnreachable);
            }
            return Ok(match labels.next_back() {
                Some("api") => {
                    if labels.next().is_some() {
                        return Err(RouterError::ExpectBrowser);
                    }
                    Endpoint::Api(self.api_server.clone())
                }
                _ => Endpoint::Http(self.http_server.clone()),
            });
        }
        Err(RouterError::BadSelector(Some(selector.to_string())))
    }
}
