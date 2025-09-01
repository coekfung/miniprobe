use std::time::Duration;

use bytes::{BufMut, Bytes, BytesMut};
use http::{Method, Request, Response, Uri, header, request, response};
use itertools::Itertools;
use log::{debug, trace};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpStream, ToSocketAddrs, lookup_host},
    task::JoinSet,
};
use tokio_native_tls::{TlsConnector as TokioTlsConnector, native_tls::TlsConnector};

const HAPPY_EYEBALLS_DELAY: Duration = Duration::from_millis(150);

pub fn basic_request_builder(uri: &str, method: Method) -> anyhow::Result<request::Builder> {
    let uri = uri.parse::<Uri>()?;
    let authority = uri
        .authority()
        .ok_or_else(|| anyhow::anyhow!("URL error: no host name"))?
        .as_str();
    let host = authority
        .find('@')
        .map(|idx| authority.split_at(idx + 1).1)
        .unwrap_or(authority);

    if host.is_empty() {
        anyhow::bail!("URL error: empty host name");
    }

    let req = Request::builder()
        .method(method)
        .header(header::HOST, host)
        .header(header::CONNECTION, "close")
        .header(header::ACCEPT_ENCODING, "identity")
        .uri(&uri);

    Ok(req)
}

pub async fn send_http_request<T: AsRef<[u8]>>(
    req: Request<T>,
    tls: bool,
    prefer_ipv6: bool,
) -> anyhow::Result<Response<Bytes>> {
    let domain = req
        .uri()
        .host()
        .ok_or_else(|| anyhow::anyhow!("URL error: no host name"))?;
    let domain = if domain.starts_with('[') {
        // IPv6 address
        &domain[1..domain.len() - 1]
    } else {
        domain
    };
    let port = req.uri().port_u16().unwrap_or(if tls { 443 } else { 80 });
    debug!("connecting to ({domain}, {port})");
    let stream = connect_happy_eyeballs((domain, port), prefer_ipv6).await?;

    trait AsyncReadWrite: AsyncRead + AsyncWrite + Unpin {}
    impl<T: AsyncRead + AsyncWrite + Unpin> AsyncReadWrite for T {}
    let mut stream: Box<dyn AsyncReadWrite> = if tls {
        let connector = TokioTlsConnector::from(TlsConnector::new()?);
        let tls_stream = connector.connect(domain, stream).await?;
        Box::new(tls_stream)
    } else {
        Box::new(stream)
    };

    stream.write_all(&assemble_http_request(req)?).await?;
    stream.flush().await?;

    let resp = {
        let mut buffer = BytesMut::with_capacity(128);
        while stream.read_buf(&mut buffer).await? != 0 {}

        let buffer = buffer.freeze();
        trace!("Response: {:?}", String::from_utf8_lossy(&buffer));
        parse_http_response(buffer)?
    };

    Ok(resp)
}

pub async fn connect_happy_eyeballs<A: ToSocketAddrs>(
    addr: A,
    prefer_ipv6: bool,
) -> anyhow::Result<TcpStream> {
    let addrs = {
        let (v4, v6): (Vec<_>, Vec<_>) = lookup_host(addr).await?.partition(|a| a.is_ipv4());

        let (first, second) = if prefer_ipv6 { (v6, v4) } else { (v4, v6) };
        first
            .into_iter()
            .interleave(second.into_iter())
            .collect::<Vec<_>>()
    };

    let mut attempts = JoinSet::new();
    let handle_attempt_result = move |res: Result<Result<TcpStream, _>, _>| match res {
        Ok(Ok(stream)) => {
            debug!(
                "connection established with {}",
                stream
                    .peer_addr()
                    .map(|a| a.to_string())
                    .unwrap_or("<unknown>".to_string())
            );
            Some(stream)
        }
        Ok(Err(e)) => {
            debug!("connection attempt failed: {e}");
            None
        }
        Err(e) => {
            debug!("connection attempt panicked: {e}");
            None
        }
    };
    for addr in addrs {
        attempts.spawn(TcpStream::connect(addr));
        while !attempts.is_empty() {
            tokio::select! {
                biased;

                res = attempts.join_next() => {
                    if let Some(stream) = handle_attempt_result(res.expect("JoinSet is not empty")) {
                        return Ok(stream);
                    }
                }
                _ = tokio::time::sleep(HAPPY_EYEBALLS_DELAY) => {
                    break;
                }
            }
        }
    }

    while let Some(res) = attempts.join_next().await {
        if let Some(stream) = handle_attempt_result(res) {
            return Ok(stream);
        }
    }

    Err(anyhow::anyhow!("I/O error: all connection attempts failed"))
}

fn assemble_http_request<T: AsRef<[u8]>>(req: Request<T>) -> anyhow::Result<Bytes> {
    let mut buffer = BytesMut::with_capacity(128);

    buffer.put_slice(
        format!(
            "{} {} {:?}\r\n",
            req.method(),
            req.uri()
                .path_and_query()
                .map(|pq| pq.as_str())
                .unwrap_or("/"),
            req.version()
        )
        .as_bytes(),
    );

    for (name, value) in req.headers() {
        buffer.put_slice(name.as_str().as_bytes());
        buffer.put_slice(b": ");
        buffer.put(value.as_bytes());
        buffer.put_slice(b"\r\n");
    }

    buffer.put_slice(b"\r\n");

    buffer.put_slice(req.body().as_ref());

    trace!("Request: {:?}", String::from_utf8_lossy(&buffer));

    Ok(buffer.freeze())
}

fn parse_http_response(bytes: Bytes) -> anyhow::Result<http::Response<Bytes>> {
    const MAX_HEADERS: usize = 64;
    let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
    let mut resp = httparse::Response::new(&mut headers);

    let status = resp.parse(&bytes)?;

    if status.is_partial() {
        anyhow::bail!("HTTP error: response is incomplete");
    }

    let body_start_index = status.unwrap();

    let response_builder = response::Builder::new()
        .status(resp.code.unwrap_or(200))
        .version(match resp.version.unwrap_or(1) {
            0 => http::Version::HTTP_10,
            1 => http::Version::HTTP_11,
            2 => http::Version::HTTP_2,
            _ => http::Version::HTTP_11,
        });

    // NOTE: headers are not used currently
    // for header in resp.headers {
    //     response_builder = response_builder.header(header.name, header.value);
    // }

    let body = bytes.slice(body_start_index..);

    Ok(response_builder.body(body)?)
}
