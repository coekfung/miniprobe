use bytes::BytesMut;
use http::{Method, header};
use miniprobe_proto::msg::{CreateSessionReq, CreateSessionResp};

use crate::{http_util, query::MetricsQuerent};
pub async fn create_session(
    token: &str,
    server_addr: &str,
    tls: bool,
    prefer_ipv6: bool,
) -> anyhow::Result<CreateSessionResp> {
    let uri = format!(
        "{}://{server_addr}/api/v1/sessions",
        if tls { "https" } else { "http" }
    );
    let body = postcard::to_extend(
        &CreateSessionReq {
            token: token.to_owned(),
            system_info: MetricsQuerent::query_static(),
        },
        BytesMut::new(),
    )?
    .freeze();
    let req = http_util::basic_request_builder(&uri, Method::POST)?
        .header(header::CONTENT_TYPE, "application/postcard")
        .header(header::CONTENT_LENGTH, body.len())
        .body(body)?;

    let resp = http_util::send_http_request(req, tls, prefer_ipv6).await?;

    if !resp.status().is_success() {
        anyhow::bail!(
            "Auth error: [{}]{}",
            resp.status().as_u16(),
            String::from_utf8_lossy(resp.body())
        );
    }

    let auth_resp: CreateSessionResp = postcard::from_bytes(resp.body())?;

    Ok(auth_resp)
}
