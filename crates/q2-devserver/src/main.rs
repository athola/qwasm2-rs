//! q2-devserver: Local development server for qwasm2-rs.
//!
//! Serves:
//! - `dist/`       → WASM build output (qwasm2.html, etc.)
//! - `gamedata/`   → Game assets (baseq2/pak0.pak, etc.)
//!
//! Adds CORS headers and serves pak0-web.pak.br with Content-Encoding: br
//! when the Brotli variant exists and the client sends Accept-Encoding: br.

use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, Response, StatusCode},
    routing::get,
    Router,
};
use std::{net::SocketAddr, path::PathBuf};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

#[derive(Clone)]
struct AppState {
    gamedata_dir: PathBuf,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8080);

    let gamedata_dir = PathBuf::from("gamedata");
    let state = AppState {
        gamedata_dir: gamedata_dir.clone(),
    };

    let app = Router::new()
        // Custom handler for web pak — serves .br variant when available + accepted
        .route("/gamedata/baseq2/pak0-web.pak", get(serve_web_pak))
        .with_state(state)
        .nest_service("/gamedata", ServeDir::new(&gamedata_dir))
        .fallback_service(ServeDir::new("dist"))
        .layer(CorsLayer::permissive());

    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    // Startup asset inventory
    log_asset(
        "gamedata/baseq2/pak0.pak",
        "source pak",
        "make gamedata-demo",
    );
    log_web_pak();

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| {
            eprintln!("Failed to bind to port {}: {}", port, e);
            eprintln!("Is another process using this port? Try: PORT=8081 make play");
            std::process::exit(1);
        });

    tracing::info!("q2-devserver listening on http://{}", addr);

    let url = format!("http://{}/qwasm2.html", addr);

    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(&url).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", &url])
            .spawn();
    }

    axum::serve(listener, app).await.unwrap();
}

fn log_asset(path: &str, label: &str, fix_hint: &str) {
    let p = std::path::Path::new(path);
    if p.exists() {
        let mb = std::fs::metadata(p)
            .map(|m| m.len() as f64 / 1_000_000.0)
            .unwrap_or(0.0);
        tracing::info!("{} ({:.1} MB) [{}]", path, mb, label);
    } else {
        tracing::warn!("{} NOT FOUND — run: {}", path, fix_hint);
    }
}

fn log_web_pak() {
    let pak = std::path::Path::new("gamedata/baseq2/pak0-web.pak");
    let br = std::path::Path::new("gamedata/baseq2/pak0-web.pak.br");
    if pak.exists() {
        let mb = std::fs::metadata(pak)
            .map(|m| m.len() as f64 / 1_000_000.0)
            .unwrap_or(0.0);
        tracing::info!(
            "gamedata/baseq2/pak0-web.pak ({:.1} MB) [web-optimized]",
            mb
        );
        if br.exists() {
            let br_mb = std::fs::metadata(br)
                .map(|m| m.len() as f64 / 1_000_000.0)
                .unwrap_or(0.0);
            tracing::info!(
                "gamedata/baseq2/pak0-web.pak.br ({:.1} MB) [brotli — served to browsers]",
                br_mb
            );
        }
    } else {
        tracing::warn!("pak0-web.pak NOT FOUND — run: make pak-web");
    }
}

/// Serve pak0-web.pak, preferring the .br variant when client accepts Brotli.
async fn serve_web_pak(State(state): State<AppState>, headers: HeaderMap) -> Response<Body> {
    let pak_path = state.gamedata_dir.join("baseq2/pak0-web.pak");
    let br_path = state.gamedata_dir.join("baseq2/pak0-web.pak.br");

    let accepts_brotli = headers
        .get("accept-encoding")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("br"))
        .unwrap_or(false);

    if accepts_brotli && br_path.exists() {
        match tokio::fs::read(&br_path).await {
            Ok(bytes) => Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "application/octet-stream")
                .header("Content-Encoding", "br")
                .header("Cache-Control", "public, max-age=3600")
                .body(Body::from(bytes))
                .unwrap(),
            Err(_) => serve_file_plain(&pak_path).await,
        }
    } else {
        serve_file_plain(&pak_path).await
    }
}

async fn serve_file_plain(path: &std::path::Path) -> Response<Body> {
    match tokio::fs::read(path).await {
        Ok(bytes) => Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "application/octet-stream")
            .body(Body::from(bytes))
            .unwrap(),
        Err(err) => {
            tracing::warn!("serve_file_plain: failed to read {}: {}", path.display(), err);
            Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::empty())
                .unwrap()
        }
    }
}
