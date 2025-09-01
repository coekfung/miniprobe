use http::{Method, header};
use miniprobe_proto::msg::{AuthReqMessage, AuthRespMessage};

use crate::{http_util, query::StatusQuerent};
pub async fn auth(
    token: &str,
    server_addr: &str,
    tls: bool,
    prefer_ipv6: bool,
) -> anyhow::Result<AuthRespMessage> {
    let uri = format!(
        "{}://{}/auth",
        if tls { "https" } else { "http" },
        server_addr
    );
    let body = postcard::to_allocvec(&AuthReqMessage {
        token: token.to_owned(),
        system_info: StatusQuerent::query_static(),
    })?;
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

    let auth_resp: AuthRespMessage = postcard::from_bytes(resp.body())?;
    log::info!("authentication successful");

    Ok(auth_resp)
}
