//! Qwasm2-rs WASM entry point.
//!
//! This crate is the `cdylib` target built by `wasm-pack build --target web`.
//! It bootstraps the engine: creates the WebGL2 context, fetches game data,
//! loads the BSP map, and runs the game loop via requestAnimationFrame.

use wasm_bindgen::prelude::*;
use web_sys::console;

#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;
#[cfg(target_arch = "wasm32")]
use std::rc::Rc;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsCast;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::JsFuture;

#[cfg(target_arch = "wasm32")]
use q2_render::gl3::Gl3Renderer;
#[cfg(target_arch = "wasm32")]
use q2_render_api::*;

// ---------------------------------------------------------------------------
// Error bridging: Q2Error → JsValue
// ---------------------------------------------------------------------------

/// Convert a Q2Error into a JsValue for WASM boundary error propagation.
#[cfg(target_arch = "wasm32")]
fn q2err_to_js(err: q2_common::Q2Error) -> JsValue {
    JsValue::from_str(&err.to_string())
}

// ---------------------------------------------------------------------------
// Engine — browser input and rendering around a local server and game
// ---------------------------------------------------------------------------

#[cfg(target_arch = "wasm32")]
struct Engine {
    renderer: Gl3Renderer,
    local: q2_server::local::LocalGame,
    /// Client-side input and view angles (C `cl.viewangles`).
    input: q2_client::input::InputState,
    canvas: web_sys::HtmlCanvasElement,
    view: ViewSize,
    last_time: f64,
}

#[cfg(target_arch = "wasm32")]
impl Engine {
    fn new(
        renderer: Gl3Renderer,
        local: q2_server::local::LocalGame,
        canvas: web_sys::HtmlCanvasElement,
        view: ViewSize,
    ) -> Self {
        Self {
            renderer,
            local,
            input: q2_client::input::InputState::default(),
            canvas,
            view,
            last_time: q2_platform::wasm::game_loop::performance_now(),
        }
    }

    /// Run one frame: build a user command from the browser input, apply it
    /// to the local player (C ClientThink), advance the server (game frames
    /// at 10 Hz), and render from the player's eye.
    fn tick(&mut self, timestamp: f64, input: &mut q2_platform::wasm::input::WasmInputState) {
        let dt = frame_dt_seconds(timestamp, self.last_time);
        self.last_time = timestamp;

        let (mouse_dx, mouse_dy) = (input.mouse_dx, input.mouse_dy);
        input.mouse_dx = 0.0;
        input.mouse_dy = 0.0;

        // C CL_ClampPitch works against the server's delta_angles.
        let delta_pitch = self
            .local
            .player()
            .map(|(_, ps)| f32::from(ps.pmove.delta_angles[0]) * (360.0 / 65536.0))
            .unwrap_or(0.0);
        turn_view(
            &mut self.input,
            &input.keys,
            mouse_dx,
            mouse_dy,
            dt,
            delta_pitch,
        );
        apply_keys(&input.keys, input.mouse_buttons, &mut self.input);
        let cmd = self.input.build_cmd(cmd_msec(dt));

        self.local.think(&cmd);
        self.local.frame((dt * 1_000_000.0) as u32);

        // The page resizes the canvas with the window; refit before drawing.
        let (canvas_w, canvas_h) = (self.canvas.width(), self.canvas.height());
        // ViewSize dimensions are always positive, so the casts are lossless.
        if (canvas_w, canvas_h) != (self.view.width as u32, self.view.height as u32) {
            match ViewSize::for_canvas(canvas_w, canvas_h) {
                Ok(Some(view)) => self.view = view,
                // Zero-area canvas: keep drawing at the last usable size.
                Ok(None) => {}
                Err(e) => console::error_1(&q2err_to_js(e)),
            }
        }

        let Some((origin, ps)) = self.local.player() else {
            return;
        };
        let fd = RefDef {
            x: 0,
            y: 0,
            width: self.view.width,
            height: self.view.height,
            fov_x: FOV_X,
            fov_y: self.view.fov_y,
            vieworg: origin + ps.viewoffset,
            viewangles: ps.viewangles + ps.kick_angles,
            time: (timestamp / 1000.0) as f32,
            ..Default::default()
        };

        self.renderer.render_frame(&fd, &[], &[], &[]);
    }
}

/// Seconds since the last frame for the movement tick, clamped to
/// `0.0..=0.05`. Negative deltas (the first RAF timestamp can precede the
/// `performance.now()` taken at engine creation) would run movement and
/// gravity backwards for a frame; long stalls are capped.
#[cfg(any(target_arch = "wasm32", test))]
fn frame_dt_seconds(timestamp_ms: f64, last_ms: f64) -> f32 {
    ((timestamp_ms - last_ms) / 1000.0).clamp(0.0, 0.05) as f32
}

/// Mouse sensitivity (degrees per pixel of mouse movement).
#[cfg(any(target_arch = "wasm32", test))]
const MOUSE_SENSITIVITY: f32 = 0.15;

/// C `cl_yawspeed` / `cl_pitchspeed` defaults: keyboard turning, deg/s.
#[cfg(any(target_arch = "wasm32", test))]
const CL_YAWSPEED: f32 = 140.0;
#[cfg(any(target_arch = "wasm32", test))]
const CL_PITCHSPEED: f32 = 150.0;

/// Move time for a user command (C CL_FinishMove): the frame time in whole
/// milliseconds, or 100 when it is over 250 ("time was unreasonable").
#[cfg(any(target_arch = "wasm32", test))]
fn cmd_msec(dt_seconds: f32) -> u8 {
    let ms = (dt_seconds * 1000.0) as i32;
    if ms > 250 {
        100
    } else {
        ms.max(0) as u8
    }
}

/// Browser key and mouse-button state into movement and button input, with
/// Quake 2's default bindings (WASD and arrows, alt+arrows strafe, space
/// jump, c crouch, shift run, ctrl or mouse1 attack).
#[cfg(any(target_arch = "wasm32", test))]
fn apply_keys(keys: &[bool; 256], mouse_buttons: u8, input: &mut q2_client::input::InputState) {
    use q2_platform::keymap::*;
    let down = |k: u8| keys[k as usize];
    let axis = |pos: bool, neg: bool| f32::from(u8::from(pos)) - f32::from(u8::from(neg));
    let strafe = down(K_ALT);
    input.forward_move = axis(
        down(b'w') || down(K_UPARROW),
        down(b's') || down(K_DOWNARROW),
    );
    input.side_move = axis(
        down(b'd') || down(b'.') || (strafe && down(K_RIGHTARROW)),
        down(b'a') || down(b',') || (strafe && down(K_LEFTARROW)),
    );
    input.jump = down(K_SPACE);
    input.crouch = down(b'c');
    input.run = down(K_SHIFT);
    input.attack = down(K_CTRL) || mouse_buttons & 1 != 0;
    input.keys = *keys;
}

/// Turn the view by mouse movement and the turn keys (C CL_AdjustAngles and
/// the mouse move), then clamp pitch as C CL_ClampPitch does against the
/// player's `delta_pitch` (degrees, already in -180..180).
#[cfg(any(target_arch = "wasm32", test))]
fn turn_view(
    input: &mut q2_client::input::InputState,
    keys: &[bool; 256],
    mouse_dx: f32,
    mouse_dy: f32,
    dt: f32,
    delta_pitch: f32,
) {
    use q2_platform::keymap::*;
    let down = |k: u8| keys[k as usize];
    let angles = &mut input.view_angles;
    angles.y -= mouse_dx * MOUSE_SENSITIVITY;
    angles.x += mouse_dy * MOUSE_SENSITIVITY;
    if !down(K_ALT) {
        if down(K_LEFTARROW) {
            angles.y += CL_YAWSPEED * dt;
        }
        if down(K_RIGHTARROW) {
            angles.y -= CL_YAWSPEED * dt;
        }
    }
    if down(K_PGDN) {
        angles.x -= CL_PITCHSPEED * dt; // +lookup
    }
    if down(K_DEL) {
        angles.x += CL_PITCHSPEED * dt; // +lookdown
    }

    if angles.x + delta_pitch < -360.0 {
        angles.x += 360.0;
    }
    if angles.x + delta_pitch > 360.0 {
        angles.x -= 360.0;
    }
    if angles.x + delta_pitch > 89.0 {
        angles.x = 89.0 - delta_pitch;
    }
    if angles.x + delta_pitch < -89.0 {
        angles.x = -89.0 - delta_pitch;
    }
}

/// Horizontal fov the view is fitted to (C `fov` cvar default).
#[cfg(any(target_arch = "wasm32", test))]
const FOV_X: f32 = 90.0;

/// Render size and the vertical fov that fits it, refreshed whenever the
/// page resizes the canvas so the picture is not stretched.
#[cfg(any(target_arch = "wasm32", test))]
#[derive(Debug, Clone, Copy, PartialEq)]
struct ViewSize {
    width: i32,
    height: i32,
    fov_y: f32,
}

#[cfg(any(target_arch = "wasm32", test))]
impl ViewSize {
    /// `Ok(None)` when the canvas has no usable area (hidden, collapsed or
    /// too large for GL's i32 viewport): there is no aspect ratio to fit.
    fn for_canvas(width: u32, height: u32) -> q2_common::error::Q2Result<Option<Self>> {
        let (Ok(width), Ok(height)) = (i32::try_from(width), i32::try_from(height)) else {
            return Ok(None);
        };
        if width == 0 || height == 0 {
            return Ok(None);
        }
        let fov_y = q2_client::view::calc_fov(FOV_X, width as f32, height as f32)?;
        Ok(Some(Self {
            width,
            height,
            fov_y,
        }))
    }
}

// ---------------------------------------------------------------------------
// WASM entry points
// ---------------------------------------------------------------------------

/// Called once from JS after `await init()`.
#[wasm_bindgen(start)]
pub fn wasm_main() {
    console_error_panic_hook_setup();
    console::log_1(&"[qwasm2-rs] WASM module initialized".into());
}

/// Get the engine version string.
#[wasm_bindgen]
pub fn engine_version() -> String {
    format!("qwasm2-rs {}", env!("CARGO_PKG_VERSION"))
}

/// Get engine info as a diagnostic string.
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

/// Run a basic engine self-test.
#[wasm_bindgen]
pub fn self_test() -> String {
    use q2_shared::types::*;

    let a = Vec3f::new(1.0, 2.0, 3.0);
    let b = Vec3f::new(4.0, 5.0, 6.0);
    let c = a + b;
    if c != Vec3f::new(5.0, 7.0, 9.0) {
        return "FAIL: Vec3 addition".to_string();
    }

    let es = EntityState::default();
    if es.number != 0 || es.origin != Vec3f::ZERO {
        return "FAIL: EntityState default".to_string();
    }

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

    "PASS".to_string()
}

/// Canvas attribute marking that an engine owns the canvas. The marker lives
/// in the DOM rather than in a Rust global, per the no-global-state rule.
#[cfg(target_arch = "wasm32")]
const ENGINE_MARK: &str = "data-qwasm2-engine";

/// Start the game engine. Called from JS after the page is ready.
/// Fetches pak0.pak, initializes WebGL2, loads the first map, and starts the game loop.
///
/// Returns `Err` if an engine was already started on this canvas: a second
/// engine would stack a second set of input listeners and a second
/// requestAnimationFrame loop on the same canvas.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub async fn start_game(canvas_id: String, pak_url: String) -> Result<(), JsValue> {
    let canvas_el = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id(&canvas_id))
        .ok_or_else(|| JsValue::from_str(&format!("no element with id '{canvas_id}'")))?;
    if canvas_el.has_attribute(ENGINE_MARK) {
        return Err(JsValue::from_str(
            "start_game: an engine is already running on this canvas",
        ));
    }
    canvas_el.set_attribute(ENGINE_MARK, "")?;

    let started = run_engine(canvas_id, pak_url).await;
    if started.is_err() {
        // Allow a retry after a failed start (e.g. the pak fetch failed).
        if let Err(e) = canvas_el.remove_attribute(ENGINE_MARK) {
            console::error_1(&e);
        }
    }
    started
}

#[cfg(target_arch = "wasm32")]
async fn run_engine(canvas_id: String, pak_url: String) -> Result<(), JsValue> {
    log("Initializing engine...");

    // 1. Create WebGL2 context
    log("Creating WebGL2 context...");
    let (gl, canvas) =
        q2_platform::wasm::gl_context::create_webgl2_context(&canvas_id).map_err(q2err_to_js)?;

    let view = ViewSize::for_canvas(canvas.width(), canvas.height())
        .map_err(q2err_to_js)?
        .ok_or_else(|| JsValue::from_str("canvas has no usable size"))?;
    let (width, height) = (view.width, view.height);
    log(&format!("Canvas: {}x{}", width, height));

    // 2. Initialize renderer
    let mut renderer = Gl3Renderer::new();
    renderer.set_gl_context(gl);
    renderer.init(width, height).map_err(q2err_to_js)?;
    log("GL3 renderer initialized");

    // 3. Fetch pak0.pak — keep bytes in JS heap; don't copy to Rust yet.
    log(&format!("Fetching {}...", pak_url));
    let pak_array = fetch_array_buffer(&pak_url).await?;
    log(&format!(
        "pak0.pak fetched: {} bytes ({:.1} MB)",
        pak_array.length(),
        pak_array.length() as f64 / (1024.0 * 1024.0)
    ));

    // 4. Index PAK directory into Rust; asset bytes stay in JS heap.
    //    Individual files are sliced from the JS Uint8Array on demand via
    //    JsPakReader — only the bytes for the current asset cross the boundary.
    let mut fs = q2_common::filesystem::FileSystem::new("baseq2");
    let reader = q2_platform::wasm::pak::JsPakReader::new(pak_array);
    let pak =
        q2_common::filesystem::Pack::open("pak0.pak", Box::new(reader)).map_err(q2err_to_js)?;
    let file_count = pak.file_count();
    fs.add_pack(pak);
    log(&format!(
        "Filesystem: {} files indexed from pak0.pak",
        file_count
    ));

    // 5. List available maps
    let maps = fs.list_files("bsp");
    let map_list: Vec<&str> = maps
        .iter()
        .filter(|m| m.starts_with("maps/"))
        .map(|s| s.as_str())
        .collect();
    log(&format!("Available maps: {}", map_list.len()));
    for m in &map_list[..map_list.len().min(5)] {
        log(&format!("  {}", m));
    }

    // 6. Load first map's BSP
    let map_name = if map_list.contains(&"maps/demo1.bsp") {
        "maps/demo1.bsp"
    } else if let Some(first) = map_list.first() {
        first
    } else {
        return Err(JsValue::from_str("No maps found in pak0.pak"));
    };

    log(&format!("Loading {}...", map_name));
    let bsp_data = fs.load_file(map_name).map_err(q2err_to_js)?;
    log(&format!("BSP loaded: {} bytes", bsp_data.len()));

    // 7. Parse BSP
    let bsp = q2_render::bsp::BspData::load(&bsp_data).map_err(q2err_to_js)?;
    log(&format!(
        "BSP parsed: {} verts, {} faces, {} texinfos, {} models",
        bsp.vertices.len(),
        bsp.faces.len(),
        bsp.texinfo.len(),
        bsp.models.len()
    ));

    // 7b. Upload BSP geometry to GPU
    renderer.load_bsp(&bsp);
    log("BSP geometry uploaded to GPU");

    // 8. Input
    let input_state =
        q2_platform::wasm::input::setup_input_listeners(&canvas).map_err(q2err_to_js)?;
    log("Input listeners attached (click canvas for pointer lock)");

    // 9. Server and game: load collision, spawn the map's entities, and put
    //    the local player in at the player start.
    let short_name = map_name
        .trim_start_matches("maps/")
        .trim_end_matches(".bsp");
    let local = q2_server::local::LocalGame::start(short_name, &bsp_data).map_err(q2err_to_js)?;
    log(&format!(
        "Game: {} entities spawned",
        local.game().entities.count()
    ));
    if let Some((origin, ps)) = local.player() {
        log(&format!(
            "Player start: ({:.0}, {:.0}, {:.0}) yaw={:.0}",
            origin.x, origin.y, origin.z, ps.viewangles.y
        ));
    }

    let engine = Engine::new(renderer, local, canvas.clone(), view);

    // 11. Start game loop
    log("Starting game loop...");
    log(&format!("Map: {} — engine running", map_name));

    let engine = Rc::new(RefCell::new(engine));
    // No catch_unwind: wasm32-unknown-unknown aborts on panic, so a frame
    // panic traps the module; the panic hook (wasm_main) logs it.
    let frame_callback = Box::new(move |timestamp: f64| {
        let mut input = input_state.borrow_mut();
        engine.borrow_mut().tick(timestamp, &mut input);
    }) as Box<dyn FnMut(f64)>;

    q2_platform::wasm::game_loop::start_game_loop(frame_callback).map_err(q2err_to_js)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Fetch a URL and return the response body as a JS-heap `Uint8Array`.
///
/// The returned array is NOT copied into Rust memory — callers decide how
/// much (if any) of the data to pull into WASM linear memory.
#[cfg(target_arch = "wasm32")]
async fn fetch_array_buffer(url: &str) -> Result<js_sys::Uint8Array, JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let resp: web_sys::Response = JsFuture::from(window.fetch_with_str(url))
        .await?
        .dyn_into()?;

    if !resp.ok() {
        return Err(JsValue::from_str(&format!(
            "Fetch failed: {} {}",
            resp.status(),
            resp.status_text()
        )));
    }

    let array_buffer = JsFuture::from(resp.array_buffer()?).await?;
    Ok(js_sys::Uint8Array::new(&array_buffer))
}

#[cfg(target_arch = "wasm32")]
fn log(msg: &str) {
    console::log_1(&format!("[qwasm2-rs] {}", msg).into());
    if let Some(window) = web_sys::window() {
        if let Some(document) = window.document() {
            if let Some(el) = document.get_element_by_id("status") {
                let current = el.inner_html();
                let escaped = msg
                    .replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;");
                el.set_inner_html(&format!(
                    "{}<span class=\"info\">{}\n</span>",
                    current, escaped
                ));
            }
        }
    }
}

fn console_error_panic_hook_setup() {
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("[qwasm2-rs PANIC] {}", info);
        web_sys::console::error_1(&msg.into());
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_msec_is_the_frame_time_with_c_unreasonable_time_rule() {
        assert_eq!(cmd_msec(0.016), 16);
        assert_eq!(cmd_msec(0.25), 250);
        // CL_FinishMove: over 250 ms "time was unreasonable", send 100.
        assert_eq!(cmd_msec(0.4), 100);
        assert_eq!(cmd_msec(-1.0), 0);
    }

    #[test]
    fn apply_keys_maps_wasd_jump_crouch_run_and_attack() {
        use q2_platform::keymap::{K_CTRL, K_SHIFT, K_SPACE};
        let mut keys = [false; 256];
        keys[b'w' as usize] = true;
        keys[b'd' as usize] = true;
        keys[K_SPACE as usize] = true;
        keys[K_SHIFT as usize] = true;
        let mut input = q2_client::input::InputState::default();
        apply_keys(&keys, 0, &mut input);
        assert_eq!((input.forward_move, input.side_move), (1.0, 1.0));
        assert!(input.jump && input.run && !input.crouch && !input.attack);

        keys[K_CTRL as usize] = true;
        apply_keys(&keys, 0, &mut input);
        assert!(input.attack, "ctrl is +attack");
        apply_keys(&[false; 256], 1, &mut input);
        assert!(input.attack, "mouse1 is +attack");
        assert_eq!(input.forward_move, 0.0);
    }

    #[test]
    fn turn_view_follows_c_mouse_signs_and_clamps_pitch() {
        let mut input = q2_client::input::InputState::default();
        // Mouse right turns right (yaw down), mouse down looks down (+pitch).
        turn_view(&mut input, &[false; 256], 10.0, 10.0, 0.0, 0.0);
        assert!(input.view_angles.y < 0.0 && input.view_angles.x > 0.0);

        // CL_ClampPitch: pitch + delta pitch stays within +-89.
        turn_view(&mut input, &[false; 256], 0.0, 100_000.0, 0.0, 0.0);
        assert_eq!(input.view_angles.x, 89.0);
        turn_view(&mut input, &[false; 256], 0.0, -100_000.0, 0.0, 10.0);
        assert_eq!(input.view_angles.x, -99.0);
    }

    #[test]
    fn turn_view_arrow_keys_turn_at_cl_yawspeed() {
        use q2_platform::keymap::K_LEFTARROW;
        let mut input = q2_client::input::InputState::default();
        let mut keys = [false; 256];
        keys[K_LEFTARROW as usize] = true;
        turn_view(&mut input, &keys, 0.0, 0.0, 0.5, 0.0);
        assert!(
            (input.view_angles.y - 70.0).abs() < 1e-4,
            "140 deg/s for 0.5 s"
        );
    }

    #[test]
    fn view_size_fits_vertical_fov_to_canvas_aspect() {
        let view = ViewSize::for_canvas(640, 480).unwrap().unwrap();
        assert_eq!((view.width, view.height), (640, 480));
        assert!((view.fov_y - 73.74).abs() < 0.01, "fov_y {}", view.fov_y);

        // A wider canvas keeps fov_x 90 and narrows fov_y, as C CalcFov does.
        let wide = ViewSize::for_canvas(1920, 1080).unwrap().unwrap();
        assert!((wide.fov_y - 58.72).abs() < 0.01, "fov_y {}", wide.fov_y);
    }

    #[test]
    fn view_size_has_no_fit_for_zero_or_unrepresentable_canvas() {
        // A hidden or collapsed canvas has no aspect ratio to fit.
        assert_eq!(ViewSize::for_canvas(0, 480).unwrap(), None);
        assert_eq!(ViewSize::for_canvas(640, 0).unwrap(), None);
        assert_eq!(ViewSize::for_canvas(u32::MAX, 480).unwrap(), None);
    }

    #[test]
    fn engine_version_contains_pkg_version() {
        let v = engine_version();
        assert!(v.starts_with("qwasm2-rs "));
        assert!(v.contains(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn engine_info_contains_expected_fields() {
        let info = engine_info();
        assert!(info.contains("protocol:"));
        assert!(info.contains("max_edicts:"));
        assert!(info.contains("max_clients:"));
        // Verify the constants match what q2_shared exports
        assert!(info.contains(&q2_shared::constants::PROTOCOL_VERSION.to_string()));
        assert!(info.contains(&q2_shared::constants::MAX_EDICTS.to_string()));
        assert!(info.contains(&q2_shared::constants::MAX_CLIENTS.to_string()));
    }

    #[test]
    fn frame_dt_is_zero_when_timestamp_precedes_last_frame() {
        // The first RAF timestamp can be earlier than performance.now()
        // taken at engine creation.
        assert_eq!(frame_dt_seconds(1000.0, 1010.0), 0.0);
    }

    #[test]
    fn frame_dt_is_capped_at_50ms() {
        assert_eq!(frame_dt_seconds(5000.0, 1000.0), 0.05);
    }

    #[test]
    fn frame_dt_converts_milliseconds_to_seconds() {
        assert!((frame_dt_seconds(1016.0, 1000.0) - 0.016).abs() < 1e-6);
    }

    #[test]
    fn self_test_passes() {
        assert_eq!(self_test(), "PASS");
    }
}
