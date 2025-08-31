use axum::{
    body::{Body, Bytes},
    extract::{FromRequest, OptionalFromRequest, Request, rejection::BytesRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use bytes::BytesMut;
use serde::{Serialize, de::DeserializeOwned};

const MIME_POSTCARD: &str = "postcard";
const MIME_APPLICATION_POSTCARD: &str = "application/postcard";

/// Postcard Exractor / Response.
#[derive(Debug, Clone, Copy, Default)]
#[must_use]
pub struct Postcard<T>(pub T);

impl<T, S> FromRequest<S> for Postcard<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = PostcardRejection;

    async fn from_request(req: Request<Body>, state: &S) -> Result<Self, Self::Rejection> {
        if !postcard_content_type(req.headers()) {
            return Err(PostcardRejection::MissingPostcardContentType);
        }

        let bytes = Bytes::from_request(req, state).await?;

        Self::from_bytes(&bytes)
    }
}

impl<T, S> OptionalFromRequest<S> for Postcard<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = PostcardRejection;

    async fn from_request(req: Request, state: &S) -> Result<Option<Self>, Self::Rejection> {
        let headers = req.headers();
        if headers.get(header::CONTENT_TYPE).is_some() {
            if postcard_content_type(headers) {
                let bytes = Bytes::from_request(req, state).await?;
                Ok(Some(Self::from_bytes(&bytes)?))
            } else {
                Err(PostcardRejection::MissingPostcardContentType)
            }
        } else {
            Ok(None)
        }
    }
}

fn postcard_content_type(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|content_type| content_type.to_str().ok())
        .and_then(|content_type| content_type.parse::<mime::Mime>().ok())
        .is_some_and(|mime| {
            mime.type_() == mime::APPLICATION
                && (mime.subtype() == MIME_POSTCARD
                    || mime.suffix().is_some_and(|name| name == MIME_POSTCARD))
        })
}

impl<T> std::ops::Deref for Postcard<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> std::ops::DerefMut for Postcard<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> From<T> for Postcard<T> {
    fn from(value: T) -> Self {
        Postcard(value)
    }
}

impl<T> Postcard<T>
where
    T: DeserializeOwned,
{
    /// Construct a `Postcard<T>` from a byte slice.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, PostcardRejection> {
        match postcard::from_bytes(&*bytes) {
            Ok(value) => Ok(Postcard(value)),
            Err(err) => Err(PostcardRejection::PostcardError(err)),
        }
    }
}

impl<T> IntoResponse for Postcard<T>
where
    T: Serialize,
{
    fn into_response(self) -> Response {
        // // TODO: maybe use 128 bytes cause serde is doing something like that
        // match postcard::to_allocvec(&self.0) {
        //     Ok(value) => (
        //         [(
        //             header::CONTENT_TYPE,
        //             HeaderValue::from_static(MIME_POSTCARD),
        //         )],
        //         value,
        //     )
        //         .into_response(),
        //     Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
        // }
        fn make_response(ser_result: postcard::Result<BytesMut>) -> Response {
            match ser_result {
                Ok(buf) => (
                    [(
                        header::CONTENT_TYPE,
                        HeaderValue::from_static(MIME_APPLICATION_POSTCARD),
                    )],
                    buf.freeze(),
                )
                    .into_response(),
                Err(err) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    [(
                        header::CONTENT_TYPE,
                        HeaderValue::from_static(mime::TEXT_PLAIN_UTF_8.as_ref()),
                    )],
                    err.to_string(),
                )
                    .into_response(),
            }
        }
        // Use a small initial capacity of 128 bytes
        let buf = BytesMut::with_capacity(128);
        let res = postcard::to_extend(&self.0, buf);
        make_response(res)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum PostcardRejection {
    #[error("Failed to parse/deserialize the request body: {0}")]
    PostcardError(#[from] postcard::Error),
    #[error("Expected request with `Content-Type: application/postcard`")]
    MissingPostcardContentType,
    #[error(transparent)]
    BytesRejection(#[from] BytesRejection),
}

impl IntoResponse for PostcardRejection {
    fn into_response(self) -> Response {
        use PostcardRejection::*;
        // its often easiest to implement `IntoResponse` by calling other implementations
        match self {
            MissingPostcardContentType => {
                (StatusCode::UNSUPPORTED_MEDIA_TYPE, self.to_string()).into_response()
            }
            PostcardError(_) => (StatusCode::BAD_REQUEST, self.to_string()).into_response(),

            BytesRejection(rejection) => rejection.into_response(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        body::to_bytes,
        http::{self, Request},
        routing::post,
    };
    use http_body_util::BodyExt;
    use serde::Deserialize;
    use tower::ServiceExt;

    #[tokio::test]
    async fn deserialize_body() {
        #[derive(Debug, Deserialize, Serialize)]
        struct Input {
            foo: String,
        }

        let app = Router::new().route(
            "/",
            post(|Postcard(input): Postcard<Input>| async { input.foo }),
        );

        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/")
            .header("content-type", "application/postcard")
            .body("\x03bar".to_string())
            .unwrap();

        let res = app.oneshot(req).await.unwrap();

        let body = res.into_body().collect().await.unwrap().to_bytes();

        assert_eq!(body, "bar");
    }

    #[tokio::test]
    async fn consume_body_to_postcard_requires_postcard_content_type() {
        #[derive(Debug, Deserialize)]
        struct Input {
            foo: String,
        }

        let app = Router::new().route(
            "/",
            post(|Postcard(input): Postcard<Input>| async { input.foo }),
        );

        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/")
            .body("\x03bar".to_string())
            .unwrap();

        let res = app.oneshot(req).await.unwrap();

        let status = res.status();

        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    #[tokio::test]
    async fn postcard_content_types() {
        async fn valid_postcard_content_type(content_type: &str) -> bool {
            println!("testing {content_type:?}");

            let app = Router::new().route("/", post(|Postcard(_): Postcard<String>| async {}));

            let req = Request::builder()
                .method(http::Method::POST)
                .uri("/")
                .header("content-type", content_type)
                .body("\x02hi".to_string())
                .unwrap();

            let res = app.oneshot(req).await.unwrap();

            res.status() == StatusCode::OK
        }

        assert!(valid_postcard_content_type("application/postcard").await);
        assert!(valid_postcard_content_type("application/postcard; charset=utf-8").await);
        assert!(valid_postcard_content_type("application/postcard;charset=utf-8").await);
        assert!(valid_postcard_content_type("application/cloudevents+postcard").await);
        assert!(!valid_postcard_content_type("text/postcard").await);
    }

    #[tokio::test]
    async fn invalid_postcard_syntax() {
        let app = Router::new().route("/", post(|_: Postcard<String>| async {}));

        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/")
            .header("content-type", "application/postcard")
            .body("\x03".to_string())
            .unwrap();

        let res = app.oneshot(req).await.unwrap();

        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[derive(Deserialize)]
    struct Foo {
        #[allow(dead_code)]
        a: i32,
        #[allow(dead_code)]
        b: Vec<Bar>,
    }

    #[derive(Deserialize)]
    struct Bar {
        #[allow(dead_code)]
        x: i32,
        #[allow(dead_code)]
        y: i32,
    }

    #[tokio::test]
    async fn invalid_postcard_data() {
        let app = Router::new().route("/", post(|_: Postcard<Foo>| async {}));

        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/")
            .header("content-type", "application/postcard")
            .body("\x02\x01\x04".to_string())
            .unwrap();

        let res = app.oneshot(req).await.unwrap();

        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let body_text = res.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(
            body_text,
            "Failed to parse/deserialize the request body: Hit the end of buffer, expected more data"
        );
    }

    #[tokio::test]
    async fn serialize_response() {
        let response = Postcard("bar").into_response();

        assert!(postcard_content_type(response.headers()));
        let bytes = &to_bytes(response.into_body(), 4).await.unwrap()[..];

        assert_eq!(bytes, b"\x03bar");
    }
}
