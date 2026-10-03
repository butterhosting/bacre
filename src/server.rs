//! The routes: an API under `/api`, and the website for everything else.

use axum::http::StatusCode;
use axum::routing::{any, get};
use axum::{Json, Router};
use serde::Serialize;

use crate::website;

pub fn router() -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/hello", get(hello))
        // an unknown API path is an error of its own, not a page of the website
        .route("/api/{*path}", any(route_not_found))
        .fallback(website::serve)
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
}

async fn health() -> Json<Health> {
    Json(Health { status: "ok" })
}

#[derive(Serialize)]
struct Hello {
    message: &'static str,
    version: &'static str,
}

async fn hello() -> Json<Hello> {
    Json(Hello {
        message: "Hello from the Rust server",
        version: env!("CARGO_PKG_VERSION"),
    })
}

#[derive(Serialize)]
struct Problem {
    error: &'static str,
}

async fn route_not_found() -> (StatusCode, Json<Problem>) {
    (
        StatusCode::NOT_FOUND,
        Json(Problem {
            error: "route_not_found",
        }),
    )
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use serde_json::{Value, json};
    use tower::ServiceExt;

    use super::router;

    async fn get(path: &str) -> (StatusCode, Value) {
        let request = Request::builder().uri(path).body(Body::empty()).unwrap();
        let response = router().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    #[tokio::test]
    async fn should_say_hello() {
        let (status, body) = get("/api/hello").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["message"], "Hello from the Rust server");
    }

    #[tokio::test]
    async fn should_report_its_health() {
        assert_eq!(
            get("/health").await,
            (StatusCode::OK, json!({ "status": "ok" }))
        );
    }

    #[tokio::test]
    async fn should_answer_an_unknown_api_path_with_json_rather_than_the_website() {
        assert_eq!(
            get("/api/nope").await,
            (StatusCode::NOT_FOUND, json!({ "error": "route_not_found" }))
        );
    }
}
