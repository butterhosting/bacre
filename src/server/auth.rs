//! Basic auth for the API: on when the daemon config names at least one user, off
//! otherwise. Hashes are bcrypt (`htpasswd -nB`).

use std::sync::Arc;

use axum::Json;
use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::json;

use crate::config::{Config, User};

#[derive(Clone)]
pub struct Users(Arc<Vec<User>>);

impl Users {
    pub fn new(config: &Config) -> Self {
        Self(Arc::new(config.users.clone()))
    }
}

pub async fn guard(State(users): State<Users>, request: Request, next: Next) -> Response {
    if !users.0.is_empty() {
        let credentials = request
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(extract_basic);
        let allowed = match credentials {
            Some((username, password)) => verify(users, username, password).await,
            None => false,
        };
        if !allowed {
            return (
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, "Basic")],
                Json(json!({ "error": "unauthorized" })),
            )
                .into_response();
        }
    }
    next.run(request).await
}

/// The username and password of a `Basic …` header; nothing for anything else
fn extract_basic(header: &str) -> Option<(String, String)> {
    let (scheme, encoded) = header.split_at_checked(6)?;
    if !scheme.eq_ignore_ascii_case("basic ") || encoded.is_empty() {
        return None;
    }
    let decoded = String::from_utf8(STANDARD.decode(encoded).ok()?).ok()?;
    // a password may contain colons; a username cannot
    let (username, password) = decoded.split_once(':')?;
    Some((username.to_string(), password.to_string()))
}

/// bcrypt is slow on purpose, so it runs off the request threads
async fn verify(users: Users, username: String, password: String) -> bool {
    tokio::task::spawn_blocking(move || {
        users.0.iter().filter(|user| user.username == username).any(|user| {
            bcrypt::verify(&password, &user.password_hash).unwrap_or_else(|e| {
                eprintln!("==> The password hash of {} could not be read; use bcrypt (htpasswd -nB): {e}", user.username);
                false
            })
        })
    })
    .await
    .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use axum::routing::get;
    use axum::{Router, middleware};
    use tower::ServiceExt;

    use super::*;
    use crate::config::testing;

    /// The layout of the real router: the API behind the guard, the health check outside it
    fn router(users: &[String]) -> Router {
        let entries: Vec<String> = users.iter().map(|entry| format!("'{entry}'")).collect();
        let config = testing::config(&format!("users: [{}]", entries.join(", ")));
        let api = Router::new()
            .route("/archives", get(|| async { "let through" }))
            .layer(middleware::from_fn_with_state(Users::new(&config), guard));
        Router::new()
            .route("/health", get(|| async { "healthy" }))
            .nest("/api", api)
    }

    fn users() -> Vec<String> {
        // the cheapest bcrypt there is; the cost is not what is under test
        [("kim", "possible"), ("shego", "a:b:c")]
            .map(|(username, password)| {
                format!("{username}:{}", bcrypt::hash(password, 4).unwrap())
            })
            .to_vec()
    }

    fn basic(username: &str, password: &str) -> Option<String> {
        Some(format!(
            "Basic {}",
            STANDARD.encode(format!("{username}:{password}"))
        ))
    }

    async fn status(
        router: Router,
        path: &str,
        authorization: Option<String>,
    ) -> (StatusCode, Option<String>) {
        let mut request = HttpRequest::builder().uri(path);
        if let Some(authorization) = authorization {
            request = request.header(header::AUTHORIZATION, authorization);
        }
        let response = router
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let challenge = response
            .headers()
            .get(header::WWW_AUTHENTICATE)
            .map(|value| value.to_str().unwrap().to_string());
        (response.status(), challenge)
    }

    #[tokio::test]
    async fn should_let_everything_through_when_there_are_no_users() {
        assert_eq!(
            status(router(&[]), "/api/archives", None).await.0,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn should_answer_per_case_when_there_are_users() {
        let users = users();
        let cases = [
            ("no credentials", "/api/archives", None, 401),
            (
                "a wrong password",
                "/api/archives",
                basic("kim", "impossible"),
                401,
            ),
            (
                "an unknown user",
                "/api/archives",
                basic("ron", "possible"),
                401,
            ),
            (
                "something that is not basic auth",
                "/api/archives",
                Some("Bearer possible".to_string()),
                401,
            ),
            (
                "the right credentials",
                "/api/archives",
                basic("kim", "possible"),
                200,
            ),
            (
                "a password with colons in it",
                "/api/archives",
                basic("shego", "a:b:c"),
                200,
            ),
            ("no credentials, on the healthcheck", "/health", None, 200),
        ];
        for (name, path, authorization, expected) in cases {
            let (status, challenge) = status(router(&users), path, authorization).await;
            assert_eq!(status.as_u16(), expected, "{name}");
            assert_eq!(
                challenge.as_deref(),
                (expected == 401).then_some("Basic"),
                "{name}"
            );
        }
    }

    #[tokio::test]
    async fn should_refuse_rather_than_fail_when_a_hash_is_in_a_format_it_cannot_verify() {
        // apache's own md5, which is what `htpasswd` writes without -B
        let users = ["kim:$apr1$lZL6V/ci$eIMz/iKDkbtys/uU7LEK00".to_string()];
        assert_eq!(
            status(router(&users), "/api/archives", basic("kim", "possible"))
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
    }

    #[test]
    fn should_read_only_well_formed_basic_headers() {
        assert_eq!(
            extract_basic(&basic("kim", "a:b").unwrap()),
            Some(("kim".to_string(), "a:b".to_string()))
        );
        assert_eq!(
            extract_basic("basic a2ltOng="),
            Some(("kim".to_string(), "x".to_string()))
        );
        for bad in ["", "Basic ", "Basic !!!", "Bearer a2ltOng=", "Basic a2lt"] {
            assert_eq!(extract_basic(bad), None, "{bad:?}");
        }
    }
}
