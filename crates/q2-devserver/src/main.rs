//! q2-devserver: Local development server for qwasm2-rs.
//!
//! Serves, relative to the repository root:
//! - `dist/`       → WASM build output (qwasm2.html, etc.)
//! - `gamedata/`   → Game assets (baseq2/pak0.pak, etc.)
//!
//! `pak0-web.pak` is served from its `.br` sibling with `Content-Encoding: br`
//! when the client accepts Brotli and the `.br` is not older than the pak.
//! The page is same-origin, so no CORS headers are sent.

use axum::{
    body::Body,
    extract::{Request, State},
    http::{header, HeaderValue, Response},
    response::IntoResponse,
    routing::get,
    Router,
};
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};
use tower::ServiceExt;
use tower_http::services::{ServeDir, ServeFile};

const DEFAULT_PORT: u16 = 8080;
const WEB_PAK: &str = "baseq2/pak0-web.pak";
const WEB_PAK_BR: &str = "baseq2/pak0-web.pak.br";

#[derive(Clone)]
struct AppState {
    gamedata_dir: PathBuf,
}

/// Repository root, so the server works from any current directory.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let port = parse_port(std::env::var("PORT").ok()).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2);
    });

    let root = repo_root();
    let app = build_app(&root);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    // Startup asset inventory
    let gamedata_dir = root.join("gamedata");
    log_asset(
        &gamedata_dir.join("baseq2/pak0.pak"),
        "source pak",
        "make gamedata-demo",
    );
    log_web_pak(&gamedata_dir);

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

/// `PORT` from the environment; unset means 8080, anything unparsable is an
/// error so the server never listens on a port other than the one requested.
fn parse_port(raw: Option<String>) -> Result<u16, String> {
    match raw {
        None => Ok(DEFAULT_PORT),
        Some(s) => s
            .parse()
            .map_err(|e| format!("invalid PORT {s:?}: {e} (expected 1-65535)")),
    }
}

fn build_app(root: &Path) -> Router {
    let gamedata_dir = root.join("gamedata");
    let state = AppState {
        gamedata_dir: gamedata_dir.clone(),
    };
    Router::new()
        .route(&format!("/gamedata/{WEB_PAK}"), get(serve_web_pak))
        .with_state(state)
        .nest_service("/gamedata", ServeDir::new(&gamedata_dir))
        .fallback_service(ServeDir::new(root.join("dist")))
}

/// Serve pak0-web.pak, using the `.br` variant only while it is fresh.
///
/// `ServeFile::precompressed_br` handles Accept-Encoding q-values and
/// conditional requests but does not send `Vary`, so it is added here; the
/// body differs by Accept-Encoding and caches must key on it.
async fn serve_web_pak(State(state): State<AppState>, req: Request) -> Response<Body> {
    let pak = state.gamedata_dir.join(WEB_PAK);
    let br = state.gamedata_dir.join(WEB_PAK_BR);

    let file = ServeFile::new(&pak);
    let file = if br_is_fresh(&pak, &br) {
        file.precompressed_br()
    } else {
        file
    };
    let Ok(resp) = file.oneshot(req).await;
    let mut resp = resp.into_response();
    resp.headers_mut().insert(
        header::VARY,
        HeaderValue::from_static(header::ACCEPT_ENCODING.as_str()),
    );
    resp
}

/// A `.br` written before its pak describes an older pak and must not be served.
fn br_is_fresh(pak: &Path, br: &Path) -> bool {
    let mtime = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified());
    match (mtime(pak), mtime(br)) {
        (Ok(pak_time), Ok(br_time)) => br_time >= pak_time,
        _ => false,
    }
}

fn log_asset(path: &Path, label: &str, fix_hint: &str) {
    match std::fs::metadata(path) {
        Ok(meta) => tracing::info!(
            "{} ({:.1} MB) [{}]",
            path.display(),
            meta.len() as f64 / 1_000_000.0,
            label
        ),
        Err(_) => tracing::warn!("{} NOT FOUND — run: {}", path.display(), fix_hint),
    }
}

fn log_web_pak(gamedata_dir: &Path) {
    let pak = gamedata_dir.join(WEB_PAK);
    let br = gamedata_dir.join(WEB_PAK_BR);
    if !pak.exists() {
        tracing::warn!("pak0-web.pak NOT FOUND — run: make pak-web");
        return;
    }
    log_asset(&pak, "web-optimized", "make pak-web");
    if br.exists() {
        if br_is_fresh(&pak, &br) {
            log_asset(&br, "brotli — served to browsers", "make pak-web");
        } else {
            tracing::warn!(
                "{} is older than pak0-web.pak and will not be served — run: make pak-web",
                br.display()
            );
        }
    }
}

#[cfg(test)]
mod tests;
