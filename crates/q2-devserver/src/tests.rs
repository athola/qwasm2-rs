use super::*;
use axum::http::{header, Request, StatusCode};
use std::time::{Duration, SystemTime};
use tower::ServiceExt;

const PAK: &[u8] = b"PACK-plain-bytes";
const BR: &[u8] = b"brotli-bytes";

/// Repo-shaped temp root: dist/qwasm2.html, gamedata/baseq2/pak0-web.pak(.br).
fn fixture_root(br_age: Option<i64>) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let baseq2 = root.path().join("gamedata/baseq2");
    std::fs::create_dir_all(&baseq2).unwrap();
    std::fs::create_dir_all(root.path().join("dist")).unwrap();
    std::fs::write(root.path().join("dist/qwasm2.html"), b"<html></html>").unwrap();

    let now = SystemTime::now();
    let pak = std::fs::File::create(baseq2.join("pak0-web.pak")).unwrap();
    std::io::Write::write_all(&mut &pak, PAK).unwrap();
    pak.set_modified(now).unwrap();
    if let Some(age_secs) = br_age {
        let br = std::fs::File::create(baseq2.join("pak0-web.pak.br")).unwrap();
        std::io::Write::write_all(&mut &br, BR).unwrap();
        // Positive age: .br written after the pak (fresh); negative: before it (stale).
        let br_time = if age_secs >= 0 {
            now + Duration::from_secs(age_secs as u64)
        } else {
            now - Duration::from_secs(age_secs.unsigned_abs())
        };
        br.set_modified(br_time).unwrap();
    }
    root
}

async fn get(root: &std::path::Path, uri: &str, headers: &[(&str, &str)]) -> Response<Body> {
    let mut req = Request::builder().uri(uri);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    build_app(root)
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn body_bytes(resp: Response<Body>) -> Vec<u8> {
    axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap()
        .to_vec()
}

const WEB_PAK: &str = "/gamedata/baseq2/pak0-web.pak";

#[tokio::test]
async fn web_pak_served_as_brotli_when_accepted_and_fresh() {
    let root = fixture_root(Some(10));
    let resp = get(root.path(), WEB_PAK, &[("accept-encoding", "gzip, br")]).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()[header::CONTENT_ENCODING], "br");
    assert_eq!(body_bytes(resp).await, BR);
}

#[tokio::test]
async fn web_pak_response_varies_on_accept_encoding() {
    let root = fixture_root(Some(10));
    for accept in ["br", "identity"] {
        let resp = get(root.path(), WEB_PAK, &[("accept-encoding", accept)]).await;
        let vary = resp
            .headers()
            .get(header::VARY)
            .map(|v| v.to_str().unwrap().to_ascii_lowercase());
        assert_eq!(vary.as_deref(), Some("accept-encoding"), "accept={accept}");
    }
}

#[tokio::test]
async fn web_pak_brotli_refused_with_q_zero() {
    let root = fixture_root(Some(10));
    let resp = get(root.path(), WEB_PAK, &[("accept-encoding", "br;q=0, gzip")]).await;
    assert!(resp.headers().get(header::CONTENT_ENCODING).is_none());
    assert_eq!(body_bytes(resp).await, PAK);
}

#[tokio::test]
async fn web_pak_plain_without_accept_encoding() {
    let root = fixture_root(Some(10));
    let resp = get(root.path(), WEB_PAK, &[]).await;
    assert!(resp.headers().get(header::CONTENT_ENCODING).is_none());
    assert_eq!(body_bytes(resp).await, PAK);
}

#[tokio::test]
async fn web_pak_stale_brotli_is_not_served() {
    let root = fixture_root(Some(-60));
    let resp = get(root.path(), WEB_PAK, &[("accept-encoding", "br")]).await;
    assert!(resp.headers().get(header::CONTENT_ENCODING).is_none());
    assert_eq!(body_bytes(resp).await, PAK);
}

#[tokio::test]
async fn web_pak_plain_when_no_brotli_variant() {
    let root = fixture_root(None);
    let resp = get(root.path(), WEB_PAK, &[("accept-encoding", "br")]).await;
    assert!(resp.headers().get(header::CONTENT_ENCODING).is_none());
    assert_eq!(body_bytes(resp).await, PAK);
}

#[tokio::test]
async fn dist_is_served_without_cors_headers() {
    let root = fixture_root(None);
    let resp = get(
        root.path(),
        "/qwasm2.html",
        &[("origin", "https://evil.example")],
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp
        .headers()
        .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
        .is_none());
}

#[test]
fn port_defaults_to_8080() {
    assert_eq!(parse_port(None).unwrap(), 8080);
}

#[test]
fn port_parses_valid_value() {
    assert_eq!(parse_port(Some("8081".into())).unwrap(), 8081);
}

#[test]
fn port_rejects_invalid_value() {
    assert!(parse_port(Some("80a".into())).is_err());
    assert!(parse_port(Some("70000".into())).is_err());
}

#[tokio::test]
async fn paths_escaping_dist_or_gamedata_are_never_served() {
    let root = fixture_root(None);
    std::fs::write(root.path().join("secret.txt"), b"SECRET").unwrap();
    for uri in [
        "/../secret.txt",
        "/%2e%2e/secret.txt",
        "/gamedata/../secret.txt",
        "/gamedata/%2e%2e/secret.txt",
        "/gamedata/..%2fsecret.txt",
        "/gamedata/baseq2/../../secret.txt",
        "//secret.txt",
    ] {
        let resp = get(root.path(), uri, &[]).await;
        assert_ne!(resp.status(), StatusCode::OK, "{uri}");
        assert!(!body_bytes(resp).await.starts_with(b"SECRET"), "{uri}");
    }
}

#[tokio::test]
async fn missing_files_are_404() {
    let root = fixture_root(None);
    for uri in ["/nope.html", "/gamedata/baseq2/nope.pak"] {
        assert_eq!(
            get(root.path(), uri, &[]).await.status(),
            StatusCode::NOT_FOUND,
            "{uri}"
        );
    }
}

#[tokio::test]
async fn head_returns_headers_without_a_body() {
    let root = fixture_root(None);
    let resp = build_app(root.path())
        .oneshot(
            Request::builder()
                .method("HEAD")
                .uri("/qwasm2.html")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(body_bytes(resp).await.is_empty());
}

#[test]
fn root_comes_from_q2_root_when_set() {
    assert_eq!(
        repo_root(Some("/srv/q2".into())),
        std::path::PathBuf::from("/srv/q2")
    );
    assert!(repo_root(None).join("crates/q2-devserver").is_dir());
}

#[test]
fn web_pak_state_reports_what_will_be_served() {
    assert_eq!(
        web_pak_state(&fixture_root(Some(10)).path().join("gamedata")),
        WebPakState::Brotli
    );
    assert_eq!(
        web_pak_state(&fixture_root(Some(-60)).path().join("gamedata")),
        WebPakState::StaleBrotli
    );
    assert_eq!(
        web_pak_state(&fixture_root(None).path().join("gamedata")),
        WebPakState::Plain
    );
    let empty = tempfile::tempdir().unwrap();
    assert_eq!(web_pak_state(empty.path()), WebPakState::Missing);
}

#[test]
fn brotli_written_in_the_same_instant_as_the_pak_is_fresh() {
    let root = fixture_root(Some(0));
    let baseq2 = root.path().join("gamedata/baseq2");
    assert!(br_is_fresh(
        &baseq2.join("pak0-web.pak"),
        &baseq2.join("pak0-web.pak.br")
    ));
}

#[test]
fn brotli_is_not_fresh_when_either_file_is_missing() {
    let root = fixture_root(Some(10));
    let baseq2 = root.path().join("gamedata/baseq2");
    let missing = baseq2.join("nope");
    assert!(!br_is_fresh(&missing, &baseq2.join("pak0-web.pak.br")));
    assert!(!br_is_fresh(&baseq2.join("pak0-web.pak"), &missing));
}

#[test]
fn server_listens_on_loopback_only() {
    let addr = listen_addr(8080);
    assert!(addr.ip().is_loopback(), "{addr}");
    assert_eq!(addr.port(), 8080);
}

#[tokio::test]
async fn other_gamedata_files_are_served_from_gamedata() {
    let root = fixture_root(None);
    std::fs::write(root.path().join("gamedata/baseq2/pak0.pak"), b"SOURCE").unwrap();
    let resp = get(root.path(), "/gamedata/baseq2/pak0.pak", &[]).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_bytes(resp).await, b"SOURCE");
}

#[tokio::test]
async fn dist_files_carry_their_mime_type() {
    // Browsers refuse WebAssembly.instantiateStreaming without application/wasm.
    let root = fixture_root(None);
    std::fs::write(root.path().join("dist/q2_wasm_bg.wasm"), b"\0asm").unwrap();
    for (uri, mime) in [
        ("/qwasm2.html", "text/html"),
        ("/q2_wasm_bg.wasm", "application/wasm"),
    ] {
        let resp = get(root.path(), uri, &[]).await;
        let content_type = resp.headers()[header::CONTENT_TYPE].to_str().unwrap();
        assert!(content_type.starts_with(mime), "{uri}: {content_type}");
    }
}

#[tokio::test]
async fn web_pak_revalidation_returns_not_modified_and_still_varies() {
    let root = fixture_root(Some(10));
    let first = get(root.path(), WEB_PAK, &[("accept-encoding", "br")]).await;
    let last_modified = first.headers()[header::LAST_MODIFIED]
        .to_str()
        .unwrap()
        .to_owned();
    let resp = get(
        root.path(),
        WEB_PAK,
        &[
            ("accept-encoding", "br"),
            ("if-modified-since", &last_modified),
        ],
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_MODIFIED);
    assert!(resp.headers().get(header::VARY).is_some());
}
