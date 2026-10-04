//! A release build embeds `website/dist`; a debug build reads it from disk on every request,
//! so a rebuilt website shows up without recompiling the server.

use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "website/dist/"]
struct Assets;

const INDEX: &str = "index.html";

pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if let Some(response) = file(path) {
        return response;
    }
    file(INDEX).unwrap_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            "The website has not been built: run `just build`, or use `just dev` and open http://localhost:3000\n",
        )
            .into_response()
    })
}

fn file(path: &str) -> Option<Response> {
    let asset = Assets::get(path)?;
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    Some(([(header::CONTENT_TYPE, mime.as_ref())], asset.data).into_response())
}
