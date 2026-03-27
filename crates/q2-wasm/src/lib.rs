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
use q2_common::player_ctrl::{MoveInput, PlayerController};
#[cfg(target_arch = "wasm32")]
use q2_render::gl3::Gl3Renderer;
#[cfg(target_arch = "wasm32")]
use q2_render_api::*;
#[cfg(target_arch = "wasm32")]
use q2_shared::types::Vec3f;

// ---------------------------------------------------------------------------
// Error bridging: Q2Error → JsValue
// ---------------------------------------------------------------------------

/// Convert a Q2Error into a JsValue for WASM boundary error propagation.
#[cfg(target_arch = "wasm32")]
fn q2err_to_js(err: q2_common::Q2Error) -> JsValue {
    JsValue::from_str(&err.to_string())
}

// ---------------------------------------------------------------------------
// Engine — thin orchestrator using PlayerController
// ---------------------------------------------------------------------------

#[cfg(target_arch = "wasm32")]
struct Engine {
    renderer: Gl3Renderer,
    collision: q2_common::collision::CollisionMap,
    player: PlayerController,
    width: i32,
    height: i32,
    last_time: f64,
}

#[cfg(target_arch = "wasm32")]
impl Engine {
    fn new(
        renderer: Gl3Renderer,
        collision: q2_common::collision::CollisionMap,
        width: i32,
        height: i32,
        spawn_pos: Vec3f,
        spawn_yaw: f32,
    ) -> Self {
        Self {
            renderer,
            collision,
            player: PlayerController::new(spawn_pos, spawn_yaw),
            width,
            height,
            last_time: q2_platform::wasm::game_loop::performance_now(),
        }
    }

    /// Run one frame: translate platform input, delegate to PlayerController, render.
    fn tick(&mut self, timestamp: f64, input: &mut q2_platform::wasm::input::WasmInputState) {
        let dt = ((timestamp - self.last_time) / 1000.0) as f32;
        self.last_time = timestamp;

        // Translate platform-specific WasmInputState → platform-agnostic MoveInput
        let mouse_dx = input.mouse_dx;
        let mouse_dy = input.mouse_dy;
        input.mouse_dx = 0.0;
        input.mouse_dy = 0.0;

        use q2_platform::keymap::*;

        let sensitivity = 0.15;
        let mut yaw_delta = -(mouse_dx * sensitivity);
        let mut pitch_delta = -(mouse_dy * sensitivity);

        // Keyboard look / turn
        let strafe_mode = input.keys[K_ALT as usize];
        if input.keys[K_LEFTARROW as usize] && !strafe_mode {
            yaw_delta += 120.0 * dt.min(0.1);
        }
        if input.keys[K_RIGHTARROW as usize] && !strafe_mode {
            yaw_delta -= 120.0 * dt.min(0.1);
        }
        if input.keys[K_DEL as usize] || input.keys[b'z' as usize] {
            pitch_delta += 80.0 * dt.min(0.1);
        }
        if input.keys[K_PGDN as usize] {
            pitch_delta -= 80.0 * dt.min(0.1);
        }

        // Movement axes
        let mut forward: f32 = 0.0;
        let mut right: f32 = 0.0;
        if input.keys[b'w' as usize] || input.keys[K_UPARROW as usize] { forward += 1.0; }
        if input.keys[b's' as usize] || input.keys[K_DOWNARROW as usize] { forward -= 1.0; }
        if input.keys[b'a' as usize] || input.keys[b',' as usize] { right -= 1.0; }
        if input.keys[b'd' as usize] || input.keys[b'.' as usize] { right += 1.0; }
        if input.keys[K_LEFTARROW as usize] && strafe_mode { right -= 1.0; }
        if input.keys[K_RIGHTARROW as usize] && strafe_mode { right += 1.0; }

        let move_input = MoveInput {
            forward,
            right,
            yaw_delta,
            pitch_delta,
            jump: input.keys[K_SPACE as usize],
            duck: input.keys[b'c' as usize],
            run: input.keys[K_SHIFT as usize],
        };

        // Delegate physics to PlayerController
        self.player.tick(dt, &move_input, &mut self.collision);

        // Render frame
        let fd = RefDef {
            x: 0,
            y: 0,
            width: self.width,
            height: self.height,
            fov_x: 90.0,
            fov_y: q2_client::view::calc_fov(90.0, self.width as f32, self.height as f32),
            vieworg: self.player.view_origin(),
            viewangles: Vec3f::new(self.player.pitch, self.player.yaw, 0.0),
            time: (timestamp / 1000.0) as f32,
            ..Default::default()
        };

        self.renderer.render_frame(&fd, &[], &[], &[]);
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

/// Start the game engine. Called from JS after the page is ready.
/// Fetches pak0.pak, initializes WebGL2, loads the first map, and starts the game loop.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub async fn start_game(canvas_id: String, pak_url: String) -> Result<(), JsValue> {
    log("Initializing engine...");

    // 1. Create WebGL2 context
    log("Creating WebGL2 context...");
    let (gl, canvas) = q2_platform::wasm::gl_context::create_webgl2_context(&canvas_id)
        .map_err(|e| JsValue::from_str(&e))?;

    let width = canvas.width() as i32;
    let height = canvas.height() as i32;
    log(&format!("Canvas: {}x{}", width, height));

    // 2. Initialize renderer
    let mut renderer = Gl3Renderer::new();
    renderer.set_gl_context(gl);
    renderer.init(width, height)
        .map_err(|e| JsValue::from_str(&format!("GL3 init failed: {e}")))?;
    log("GL3 renderer initialized");

    // 3. Fetch pak0.pak
    log(&format!("Fetching {}...", pak_url));
    let pak_data = fetch_bytes(&pak_url).await?;
    log(&format!("pak0.pak loaded: {} bytes ({:.1} MB)",
        pak_data.len(), pak_data.len() as f64 / (1024.0 * 1024.0)));

    // 4. Load PAK into virtual filesystem
    let mut fs = q2_common::filesystem::FileSystem::new("baseq2");
    let pak = q2_common::filesystem::Pack::load_from_bytes("pak0.pak", &pak_data)
        .map_err(q2err_to_js)?;
    let file_count = pak.files.len();
    fs.add_pack(pak);
    log(&format!("Filesystem: {} files from pak0.pak", file_count));

    // 5. List available maps
    let maps = fs.list_files("bsp");
    let map_list: Vec<&str> = maps.iter()
        .filter(|m| m.starts_with("maps/"))
        .map(|s| s.as_str())
        .collect();
    log(&format!("Available maps: {}", map_list.len()));
    for m in &map_list[..map_list.len().min(5)] {
        log(&format!("  {}", m));
    }

    // 6. Load first map's BSP
    let map_name = if map_list.iter().any(|m| *m == "maps/demo1.bsp") {
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
    let bsp = q2_render::bsp::BspData::load(&bsp_data)
        .map_err(|e| JsValue::from_str(&format!("BSP parse error: {}", e)))?;
    log(&format!("BSP parsed: {} verts, {} faces, {} texinfos, {} models",
        bsp.vertices.len(), bsp.faces.len(), bsp.texinfo.len(), bsp.models.len()));

    // 7b. Upload BSP geometry to GPU
    renderer.load_bsp(&bsp);
    log("BSP geometry uploaded to GPU");

    // 8. Load collision map
    let mut collision = q2_common::collision::CollisionMap::new();
    collision.load_map(&bsp_data).map_err(q2err_to_js)?;
    log(&format!("Collision map loaded: {} models, {} brushes, {} nodes, {} leafs, {} planes",
        collision.num_models(),
        collision.num_brushes(),
        collision.num_nodes(),
        collision.num_leafs(),
        collision.num_planes(),
    ));

    // 9. Set up input
    let input_state = q2_platform::wasm::input::setup_input_listeners(&canvas)
        .map_err(|e| JsValue::from_str(&e))?;
    log("Input listeners attached (click canvas for pointer lock)");

    // 10. Find player start via game spawn system
    let (spawn_pos, spawn_yaw) = q2_game::spawn::find_player_start(&bsp.entities)
        .unwrap_or((Vec3f::ZERO, 0.0));
    log(&format!("Player start: ({:.0}, {:.0}, {:.0}) yaw={:.0}",
        spawn_pos.x, spawn_pos.y, spawn_pos.z, spawn_yaw));

    let mut engine = Engine::new(renderer, collision, width, height, spawn_pos, spawn_yaw);
    engine.player.snap_to_ground(&mut engine.collision);

    // 11. Start game loop
    log("Starting game loop...");
    log(&format!("Map: {} — engine running", map_name));

    let engine = Rc::new(RefCell::new(engine));
    let frame_callback = Box::new(move |timestamp: f64| {
        let mut input = input_state.borrow_mut();
        engine.borrow_mut().tick(timestamp, &mut input);
    }) as Box<dyn FnMut(f64)>;

    q2_platform::wasm::game_loop::start_game_loop(frame_callback)
        .map_err(|e| JsValue::from_str(&e))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Fetch a URL as raw bytes.
#[cfg(target_arch = "wasm32")]
async fn fetch_bytes(url: &str) -> Result<Vec<u8>, JsValue> {
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
    let uint8_array = js_sys::Uint8Array::new(&array_buffer);
    Ok(uint8_array.to_vec())
}

#[cfg(target_arch = "wasm32")]
fn log(msg: &str) {
    console::log_1(&format!("[qwasm2-rs] {}", msg).into());
    if let Some(window) = web_sys::window() {
        if let Some(document) = window.document() {
            if let Some(el) = document.get_element_by_id("status") {
                let current = el.inner_html();
                let escaped = msg.replace('&', "&amp;")
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
    fn self_test_passes() {
        assert_eq!(self_test(), "PASS");
    }
}
