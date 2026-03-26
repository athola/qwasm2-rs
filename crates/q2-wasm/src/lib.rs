//! Qwasm2-rs WASM entry point.
//!
//! This crate is the `cdylib` target built by `wasm-pack build --target web`.
//! It exposes `#[wasm_bindgen]` functions that the HTML/JS bootstrap calls.

use wasm_bindgen::prelude::*;
use web_sys::console;

/// Called once from JS after `await init()`. Sets up the engine.
#[wasm_bindgen(start)]
pub fn wasm_main() {
    // Set up panic hook for better error messages in browser console
    console_error_panic_hook_setup();

    console::log_1(&"[qwasm2-rs] WASM module initialized".into());
}

/// Get the engine version string.
#[wasm_bindgen]
pub fn engine_version() -> String {
    format!("qwasm2-rs {}", env!("CARGO_PKG_VERSION"))
}

/// Get engine info as a diagnostic string (useful for Playwright tests).
#[wasm_bindgen]
pub fn engine_info() -> String {
    format!(
        "qwasm2-rs v{}\nprotocol: {}\nmax_edicts: {}\nmax_clients: {}",
        env!("CARGO_PKG_VERSION"),
        q2_shared::constants::PROTOCOL_VERSION,
        q2_shared::constants::MAX_EDICTS,
        q2_shared::constants::MAX_CLIENTS,
    )
}

/// Check WebGL2 support and return diagnostic info.
#[wasm_bindgen]
pub fn check_webgl2() -> String {
    let window = web_sys::window().expect("no window");
    let document = window.document().expect("no document");

    // Try to create a temporary canvas to test WebGL2
    let canvas = document
        .create_element("canvas")
        .expect("can't create canvas");
    let canvas: web_sys::HtmlCanvasElement = canvas.dyn_into().expect("not a canvas");

    match canvas.get_context("webgl2") {
        Ok(Some(_ctx)) => "WebGL2: supported".to_string(),
        Ok(None) => "WebGL2: context returned None".to_string(),
        Err(e) => format!("WebGL2: error — {:?}", e),
    }
}

/// Run a basic engine self-test. Returns "PASS" or error description.
/// Used by Playwright to verify the WASM module works.
#[wasm_bindgen]
pub fn self_test() -> String {
    use q2_shared::types::*;

    // Test 1: Vec3 math
    let a = Vec3f::new(1.0, 2.0, 3.0);
    let b = Vec3f::new(4.0, 5.0, 6.0);
    let c = a + b;
    if c != Vec3f::new(5.0, 7.0, 9.0) {
        return "FAIL: Vec3 addition".to_string();
    }

    // Test 2: EntityState default
    let es = EntityState::default();
    if es.number != 0 || es.origin != Vec3f::ZERO {
        return "FAIL: EntityState default".to_string();
    }

    // Test 3: NetMsg roundtrip
    let mut buf = q2_common::net_msg::NetMsg::new();
    buf.write_byte(42);
    buf.write_short(1234);
    buf.write_string("hello");
    buf.begin_reading();
    if buf.read_byte() != 42 {
        return "FAIL: NetMsg read_byte".to_string();
    }
    if buf.read_short() != 1234 {
        return "FAIL: NetMsg read_short".to_string();
    }
    if buf.read_string() != "hello" {
        return "FAIL: NetMsg read_string".to_string();
    }

    // Test 4: Error system
    let err = q2_common::error::Q2Error::Drop("test".into());
    if !err.is_recoverable() {
        return "FAIL: Q2Error::Drop should be recoverable".to_string();
    }

    "PASS".to_string()
}

/// Simple console.error panic hook (no extra dependency needed).
fn console_error_panic_hook_setup() {
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("[qwasm2-rs PANIC] {}", info);
        web_sys::console::error_1(&msg.into());
    }));
}
