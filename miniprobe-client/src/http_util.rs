use bytes::{BufMut, Bytes, BytesMut};
use http::{Request, response};
use log::trace;

pub fn generate_http_request<T: AsRef<[u8]>>(req: Request<T>) -> anyhow::Result<Bytes> {
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

pub fn parse_http_response(bytes: &[u8]) -> anyhow::Result<http::Response<Vec<u8>>> {
    const MAX_HEADERS: usize = 64;
    let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
    let mut resp = httparse::Response::new(&mut headers);

    let status = resp.parse(bytes)?;

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

    let body = bytes[body_start_index..].to_vec();

    Ok(response_builder.body(body)?)
}
