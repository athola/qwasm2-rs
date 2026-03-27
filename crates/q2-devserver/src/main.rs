//! q2-devserver: Local development server for qwasm2-rs.
//!
//! Serves:
//! - `dist/`       → WASM build output (qwasm2.html, etc.)
//! - `gamedata/`   → Game assets (baseq2/pak0.pak, etc.)
//!
//! Adds correct MIME types for .wasm and CORS headers for local dev.

use axum::Router;
use std::net::SocketAddr;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8080);

    // Serve dist/ at root, gamedata/ at /gamedata/
    let app = Router::new()
        .nest_service("/gamedata", ServeDir::new("gamedata"))
        .fallback_service(ServeDir::new("dist"))
        .layer(CorsLayer::permissive());

    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    let pak_path = std::path::Path::new("gamedata/baseq2/pak0.pak");
    if pak_path.exists() {
        let size = std::fs::metadata(pak_path)
            .map(|m| m.len() / (1024 * 1024))
            .unwrap_or(0);
        tracing::info!("gamedata/baseq2/pak0.pak found ({} MB)", size);
    } else {
        tracing::warn!("gamedata/baseq2/pak0.pak NOT FOUND — run: make gamedata-demo");
    }

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap_or_else(|e| {
        eprintln!("Failed to bind to port {}: {}", port, e);
        eprintln!("Is another process using this port? Try: PORT=8081 make play");
        std::process::exit(1);
    });

    tracing::info!("q2-devserver listening on http://{}", addr);

    let url = format!("http://{}/qwasm2.html", addr);

    // Auto-open browser (after bind succeeds so the server is ready)
    #[cfg(target_os = "macos")]
    { let _ = std::process::Command::new("open").arg(&url).spawn(); }
    #[cfg(target_os = "linux")]
    { let _ = std::process::Command::new("xdg-open").arg(&url).spawn(); }
    #[cfg(target_os = "windows")]
    { let _ = std::process::Command::new("cmd").args(["/C", "start", &url]).spawn(); }

    axum::serve(listener, app).await.unwrap();
}
