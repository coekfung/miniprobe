use std::net::ToSocketAddrs;

use bytes::BytesMut;
use http::{Method, Request, Uri, header};
use log::{debug, trace};
use miniprobe_proto::msg::{AuthReqMessage, AuthRespMessage};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpStream,
};
use tokio_native_tls::{TlsConnector as TokioTlsConnector, native_tls::TlsConnector};

use crate::{http_util, query::StatusQuerent};
pub async fn auth(token: &str, server_addr: &str, tls: bool) -> anyhow::Result<AuthRespMessage> {
    let uri = format!(
        "{}://{}/auth",
        if tls { "https" } else { "http" },
        server_addr
    )
    .parse::<Uri>()?;
    let authority = uri
        .authority()
        .ok_or_else(|| anyhow::anyhow!("URL error: no host name"))?
        .as_str();
    let header_host = authority
        .find('@')
        .map(|idx| authority.split_at(idx + 1).1)
        .unwrap_or(authority);

    if header_host.is_empty() {
        anyhow::bail!("URL error: empty host name");
    }

    let body = postcard::to_allocvec(&AuthReqMessage {
        token: token.to_owned(),
        system_info: StatusQuerent::query_static(),
    })?;
    let req = Request::builder()
        .method(Method::POST)
        .header(header::HOST, header_host)
        .header(header::CONNECTION, "close")
        .header(header::ACCEPT_ENCODING, "identity")
        .header(header::CONTENT_TYPE, "application/postcard")
        .header(header::CONTENT_LENGTH, body.len())
        .uri(&uri)
        .body(body)?;

    let host = uri
        .host()
        .ok_or_else(|| anyhow::anyhow!("URL error: no host name"))?;
    let host = if host.starts_with('[') {
        // IPv6 address
        &host[1..host.len() - 1]
    } else {
        host
    };
    let port = uri.port_u16().unwrap_or(if tls { 443 } else { 80 });
    debug!("Looking up address information for ({host}, {port})");
    let addrs = (host, port).to_socket_addrs()?;

    let stream = async {
        for addr in addrs {
            debug!("Trying to contact {uri} at {addr}...");
            if let Ok(s) = TcpStream::connect(addr).await {
                return Ok(s);
            }
        }
        Err(anyhow::anyhow!("Url error: unable to connect {uri}"))
    }
    .await?;

    trait AsyncReadWrite: AsyncRead + AsyncWrite + Unpin {}
    impl<T: AsyncRead + AsyncWrite + Unpin> AsyncReadWrite for T {}
    let mut stream: Box<dyn AsyncReadWrite> = if tls {
        let connector = TokioTlsConnector::from(TlsConnector::new()?);
        let tls_stream = connector.connect(host, stream).await?;
        Box::new(tls_stream)
    } else {
        Box::new(stream)
    };

    stream
        .write_all(&http_util::generate_http_request(req)?)
        .await?;
    stream.flush().await?;

    let resp = {
        let mut buffer = BytesMut::with_capacity(128);
        while stream.read_buf(&mut buffer).await? != 0 {}
        trace!("Response: {:?}", String::from_utf8_lossy(&buffer));

        http_util::parse_http_response(&buffer)?
    };

    if !resp.status().is_success() {
        anyhow::bail!(
            "authentication failed with: [{}]{}",
            resp.status().as_u16(),
            String::from_utf8_lossy(resp.body())
        );
    }

    let auth_resp: AuthRespMessage = postcard::from_bytes(resp.body())?;
    log::info!("authentication successful");

    Ok(auth_resp)
}
