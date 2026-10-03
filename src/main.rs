mod server;
mod website;

use std::net::SocketAddr;

use tokio::net::TcpListener;
use tokio::signal;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let address = SocketAddr::from(([127, 0, 0, 1], 3001));
    let listener = TcpListener::bind(address).await?;
    println!("==> Bacre listening on http://{address}");

    axum::serve(listener, server::router())
        .with_graceful_shutdown(shutdown())
        .await
}

/// Resolves on Ctrl-C or SIGTERM, so open requests can finish before the process leaves
async fn shutdown() {
    let mut terminate = signal::unix::signal(signal::unix::SignalKind::terminate())
        .expect("could not listen for SIGTERM");
    tokio::select! {
        _ = signal::ctrl_c() => {}
        _ = terminate.recv() => {}
    }
}
