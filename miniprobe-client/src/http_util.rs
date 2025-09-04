use std::{pin::Pin, time::Duration};

use bytes::{BufMut, Bytes, BytesMut};
use http::{Method, Request, Response, Uri, header, request, response};
use itertools::Itertools;
use log::{debug, trace};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpStream, ToSocketAddrs, lookup_host},
    task::JoinSet,
};
use tokio_native_tls::{TlsConnector as TokioTlsConnector, TlsStream, native_tls::TlsConnector};

const HAPPY_EYEBALLS_DELAY: Duration = Duration::from_millis(150);

pub enum MaybeTlsStream<S> {
    Plain(S),
    Tls(TlsStream<S>),
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncRead for MaybeTlsStream<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            MaybeTlsStream::Plain(s) => Pin::new(s).poll_read(cx, buf),
            MaybeTlsStream::Tls(s) => Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWrite for MaybeTlsStream<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match self.get_mut() {
            MaybeTlsStream::Plain(s) => Pin::new(s).poll_write(cx, buf),
            MaybeTlsStream::Tls(s) => Pin::new(s).poll_write(cx, buf),
        }
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            MaybeTlsStream::Plain(s) => Pin::new(s).poll_flush(cx),
            MaybeTlsStream::Tls(s) => Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            MaybeTlsStream::Plain(s) => Pin::new(s).poll_shutdown(cx),
            MaybeTlsStream::Tls(s) => Pin::new(s).poll_shutdown(cx),
        }
    }
}

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
    let stream = &mut connect_tls(&req, tls, prefer_ipv6).await?;

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

pub async fn connect_tls<T>(
    req: &Request<T>,
    tls: bool,
    prefer_ipv6: bool,
) -> anyhow::Result<MaybeTlsStream<TcpStream>> {
    let domain = req
        .uri()
        .host()
        .ok_or_else(|| anyhow::anyhow!("URL error: no host name"))?;
    let port = req.uri().port_u16().unwrap_or(if tls { 443 } else { 80 });
    trace!("connecting to ({domain}, {port})");
    let stream = connect_happy_eyeballs((domain, port), prefer_ipv6).await?;

    let stream = if tls {
        let connector = TokioTlsConnector::from(TlsConnector::new()?);
        let tls_stream = connector.connect(domain, stream).await?;
        MaybeTlsStream::Tls(tls_stream)
    } else {
        MaybeTlsStream::Plain(stream)
    };

    Ok(stream)
}

async fn connect_happy_eyeballs<A: ToSocketAddrs>(
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
            trace!("connection attempt failed: {e}");
            None
        }
        Err(e) => {
            trace!("connection attempt panicked: {e}");
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

    let mut response_builder = response::Builder::new()
        .status(resp.code.unwrap_or(200))
        .version(match resp.version.unwrap_or(1) {
            0 => http::Version::HTTP_10,
            1 => http::Version::HTTP_11,
            2 => http::Version::HTTP_2,
            _ => http::Version::HTTP_11,
        });

    for header in resp.headers {
        response_builder = response_builder.header(header.name, header.value);
    }

    let body = bytes.slice(body_start_index..);

    Ok(response_builder.body(body)?)
}
