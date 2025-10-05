use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::router::RouterError;

const SOCKS_VERSION5: u8 = 5;

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub struct AuthenticationMethod(u8);

pub const NO_AUTHENTICATION: AuthenticationMethod = AuthenticationMethod(0);
pub const NO_ACCEPTABLE_AUTHENTICATION: AuthenticationMethod = AuthenticationMethod(255);

impl From<AuthenticationMethod> for u8 {
    fn from(v: AuthenticationMethod) -> u8 {
        v.0
    }
}

const COMMAND_CONNECT: u8 = 1;

const ADDRTYPE_V4: u8 = 1;
const ADDRTYPE_DOMAIN: u8 = 3;
const ADDRTYPE_V6: u8 = 4;

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub struct SocksReply(u8);

impl From<SocksReply> for u8 {
    fn from(v: SocksReply) -> u8 {
        v.0
    }
}

pub const REP_SUCCEEDED: SocksReply = SocksReply(0);
pub const REP_CONNECTION_NOT_ALLOWED: SocksReply = SocksReply(2);
pub const REP_HOST_NOT_REACHABLE: SocksReply = SocksReply(4);
pub const REP_CONNECTION_REFUSED: SocksReply = SocksReply(5);
pub const REP_COMMAND_NOT_SUPPORTED: SocksReply = SocksReply(7);
pub const REP_ADDRESS_TYPE_NOT_SUPPORTED: SocksReply = SocksReply(8);

impl From<RouterError> for SocksReply {
    fn from(re: RouterError) -> SocksReply {
        match re {
            RouterError::IOError(ioe) => match ioe.kind() {
                std::io::ErrorKind::PermissionDenied => REP_CONNECTION_NOT_ALLOWED,
                _ => REP_HOST_NOT_REACHABLE,
            },
            RouterError::UnrecognisedSuffix(_, _) => REP_CONNECTION_NOT_ALLOWED,
            RouterError::BadSelector(_) => REP_HOST_NOT_REACHABLE,
            RouterError::ExpectKubeObject => REP_HOST_NOT_REACHABLE,
            RouterError::BadServerIdentity => REP_HOST_NOT_REACHABLE,
            RouterError::ExpectBrowser => REP_HOST_NOT_REACHABLE,
            RouterError::UriError(_) => REP_HOST_NOT_REACHABLE,
            RouterError::AddMtls(_) => REP_HOST_NOT_REACHABLE,
            RouterError::PortUnreachable => REP_CONNECTION_REFUSED,
        }
    }
}

#[derive(Debug, Error)]
pub enum SocksProtocolError {
    #[error("{0}")]
    IOError(#[from] std::io::Error),
    #[error("{0}")]
    Utf8(#[from] std::string::FromUtf8Error),
    #[error("Wrong protocol version, expected SOCKS_VERSION5, got {0}")]
    WrongVersion(u8),
    #[error("Socks address type {0} is not supported")]
    WrongAddrType(u8),
}

pub async fn read_client_hello<T>(
    s: &mut T,
) -> Result<Vec<AuthenticationMethod>, SocksProtocolError>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    let mut header: [u8; 2] = [0; 2];
    let _ = s.read_exact(&mut header).await?;
    if header[0] != SOCKS_VERSION5 {
        return Err(SocksProtocolError::WrongVersion(header[0]));
    }
    let mut buffer: [u8; 256] = [0; 256];
    let method_count = usize::from(header[1]);
    let _ = s.read_exact(&mut buffer[..method_count]).await?;
    Ok(buffer
        .into_iter()
        .take(method_count)
        .map(AuthenticationMethod)
        .collect())
}

pub async fn server_hello_no_auth<T: AsyncWrite + Unpin>(s: &mut T) -> Result<(), std::io::Error> {
    let response: [u8; 2] = [SOCKS_VERSION5, NO_AUTHENTICATION.into()];
    s.write_all(&response).await
}

pub async fn server_hello_no_acceptable_auth<T: AsyncWrite + Unpin>(
    s: &mut T,
) -> Result<(), std::io::Error> {
    let response: [u8; 2] = [SOCKS_VERSION5, NO_ACCEPTABLE_AUTHENTICATION.into()];
    s.write_all(&response).await
}

#[derive(Debug)]
pub enum SocksAddr {
    Domain(String),
    IP,
}

#[derive(Debug)]
pub enum SocksRequest {
    Connect(SocksAddr, u16),
    Other,
}

pub async fn read_socks_request<T>(s: &mut T) -> Result<SocksRequest, SocksProtocolError>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    let mut header: [u8; 4] = [0; 4];
    let _ = s.read_exact(&mut header).await?;
    if header[0] != SOCKS_VERSION5 {
        return Err(SocksProtocolError::WrongVersion(header[0]));
    }
    let (addr, port) = match header[3] {
        ADDRTYPE_DOMAIN => {
            let domain_length = usize::from(s.read_u8().await?);
            let mut buffer: [u8; 257] = [0; 257];
            let _ = s.read_exact(&mut buffer[..(domain_length + 2)]).await?;
            let domain = String::from_utf8(buffer[..domain_length].to_vec())?;
            (
                SocksAddr::Domain(domain),
                u16::from_be_bytes([buffer[domain_length], buffer[domain_length + 1]]),
            )
        }
        ADDRTYPE_V4 => {
            let mut b: [u8; 6] = [0; 6];
            let _ = s.read_exact(&mut b).await?;
            (SocksAddr::IP, u16::from_be_bytes([b[4], b[5]]))
        }
        ADDRTYPE_V6 => {
            let mut b: [u8; 18] = [0; 18];
            let _ = s.read_exact(&mut b).await?;
            (SocksAddr::IP, u16::from_be_bytes([b[4], b[5]]))
        }
        x => {
            return Err(SocksProtocolError::WrongAddrType(x));
        }
    };
    Ok(match header[1] {
        COMMAND_CONNECT => SocksRequest::Connect(addr, port),
        _ => SocksRequest::Other,
    })
}

pub async fn send_reply<T: AsyncWrite + Unpin>(
    s: &mut T,
    reply: SocksReply,
) -> Result<(), std::io::Error> {
    let reply = [
        SOCKS_VERSION5,
        reply.into(),
        0,
        ADDRTYPE_V4,
        0,
        0,
        0,
        0,
        0,
        0,
    ];
    s.write_all(&reply).await
}
