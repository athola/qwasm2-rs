//! Qwasm2-rs WASM entry point.
//!
//! This crate is the `cdylib` target built by `wasm-pack build --target web`.
//! It bootstraps the engine: creates the WebGL2 context, fetches game data,
//! loads the BSP map, and runs the game loop via requestAnimationFrame.

use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::console;

use q2_render::gl3::Gl3Renderer;
use q2_render_api::*;

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
    if !renderer.init(width, height) {
        return Err(JsValue::from_str("Failed to initialize GL3 renderer"));
    }
    log("GL3 renderer initialized");

    // 3. Fetch pak0.pak
    log(&format!("Fetching {}...", pak_url));
    let pak_data = fetch_bytes(&pak_url).await?;
    log(&format!("pak0.pak loaded: {} bytes ({:.1} MB)",
        pak_data.len(), pak_data.len() as f64 / (1024.0 * 1024.0)));

    // 4. Load PAK into virtual filesystem
    let mut fs = q2_common::filesystem::FileSystem::new("baseq2");
    let pak = q2_common::filesystem::Pack::load_from_bytes("pak0.pak", &pak_data)
        .map_err(|e| JsValue::from_str(&format!("PAK load error: {}", e)))?;
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
    let bsp_data = fs.load_file(map_name)
        .map_err(|e| JsValue::from_str(&format!("BSP load error: {}", e)))?;
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
    collision.load_map(&bsp_data)
        .map_err(|e| JsValue::from_str(&format!("Collision load error: {}", e)))?;
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

    // 10. Start game loop
    log("Starting game loop...");
    log(&format!("Map: {} — engine running", map_name));

    let renderer = Rc::new(RefCell::new(renderer));
    let collision = Rc::new(RefCell::new(collision));
    let input_state_clone = input_state.clone();
    let mut last_time = q2_platform::wasm::game_loop::performance_now();
    let mut camera_yaw: f32 = 0.0;
    let mut camera_pitch: f32 = 0.0;
    let mut camera_pos = q2_shared::types::Vec3f::ZERO;
    let mut velocity_z: f32 = 0.0;
    let mut on_ground: bool = false;

    // Q2 player bbox
    let player_mins_stand = q2_shared::types::Vec3f::new(-16.0, -16.0, -24.0);
    let player_maxs_stand = q2_shared::types::Vec3f::new(16.0, 16.0, 32.0);
    let player_mins_duck = q2_shared::types::Vec3f::new(-16.0, -16.0, -24.0);
    let player_maxs_duck = q2_shared::types::Vec3f::new(16.0, 16.0, 4.0);
    let mut ducked = false;

    // Q2 physics constants
    const GRAVITY: f32 = 800.0;
    const JUMP_VELOCITY: f32 = 270.0;
    const MOVE_SPEED: f32 = 300.0;
    // CONTENTS_SOLID
    const MASK_PLAYERSOLID: i32 = 1 | 0x10000; // CONTENTS_SOLID | CONTENTS_WINDOW

    // Find player start position from BSP entities
    if let Some((pos, angle)) = find_player_start(&bsp.entities) {
        camera_pos = pos;
        camera_yaw = angle;
        log(&format!("Player start: ({:.0}, {:.0}, {:.0}) yaw={:.0}", pos.x, pos.y, pos.z, angle));
    }

    // Diagnostic: test trace straight down from player start
    {
        let mut cm = collision.borrow_mut();
        let start = camera_pos;
        let end = q2_shared::types::Vec3f::new(camera_pos.x, camera_pos.y, camera_pos.z - 1000.0);
        let trace = cm.box_trace(start, end, player_mins_stand, player_maxs_stand, 0, MASK_PLAYERSOLID);
        log(&format!("Ground trace test: fraction={:.4}, allsolid={}, startsolid={}, endpos=({:.1}, {:.1}, {:.1})",
            trace.fraction, trace.allsolid, trace.startsolid,
            trace.endpos.x, trace.endpos.y, trace.endpos.z));
        // If trace found ground, snap to it
        if trace.fraction < 1.0 {
            camera_pos = trace.endpos;
            on_ground = true;
            log(&format!("Snapped to ground: ({:.0}, {:.0}, {:.0})", camera_pos.x, camera_pos.y, camera_pos.z));
        } else {
            log("WARNING: no ground found below player start — collision may not be working");
            // Try with just CONTENTS_SOLID = 1
            let trace2 = cm.box_trace(start, end, player_mins_stand, player_maxs_stand, 0, 1);
            log(&format!("Trace with mask=1: fraction={:.4}", trace2.fraction));
            // Try point trace (no bbox)
            let trace3 = cm.box_trace(start, end, q2_shared::types::Vec3f::ZERO, q2_shared::types::Vec3f::ZERO, 0, 1);
            log(&format!("Point trace mask=1: fraction={:.4}", trace3.fraction));
            // Check point contents at start
            let contents = cm.point_contents(start, 0);
            log(&format!("Contents at start: {}", contents));
        }
    }

    let frame_callback = Box::new(move |timestamp: f64| {
        let dt = ((timestamp - last_time) / 1000.0) as f32;
        last_time = timestamp;
        let dt = dt.min(0.1);

        // Read input
        let mut input = input_state_clone.borrow_mut();
        let mouse_dx = input.mouse_dx;
        let mouse_dy = input.mouse_dy;
        input.mouse_dx = 0.0;
        input.mouse_dy = 0.0;

        use q2_platform::keymap::*;

        // Mouse look (always on — Q2's default +mlook via \ key)
        let sensitivity = 0.15;
        camera_yaw -= mouse_dx * sensitivity;
        camera_pitch -= mouse_dy * sensitivity;
        camera_pitch = camera_pitch.clamp(-89.0, 89.0);

        // Run modifier: SHIFT doubles speed (default.cfg: bind SHIFT +speed)
        let run = input.keys[K_SHIFT as usize];
        let move_speed = if run { MOVE_SPEED * 2.0 } else { MOVE_SPEED };
        let speed = move_speed * dt;

        let yaw_rad = camera_yaw.to_radians();
        let forward = q2_shared::types::Vec3f::new(yaw_rad.cos(), yaw_rad.sin(), 0.0);
        let right = q2_shared::types::Vec3f::new(-yaw_rad.sin(), yaw_rad.cos(), 0.0);

        let mut wish = q2_shared::types::Vec3f::ZERO;

        // default.cfg movement bindings:
        //   UPARROW / W    → +forward
        //   DOWNARROW / S  → +back
        //   LEFTARROW      → +left (turn)
        //   RIGHTARROW     → +right (turn)
        //   ,              → +moveleft (strafe)
        //   .              → +moveright (strafe)
        //   A / D          → strafe (WASD-style, modern addition)
        //
        // We map both classic and WASD:
        // Forward/back
        if input.keys[b'w' as usize] || input.keys[K_UPARROW as usize] {
            wish += forward;
        }
        if input.keys[b's' as usize] || input.keys[K_DOWNARROW as usize] {
            wish -= forward;
        }
        // Strafe left/right (WASD + comma/period)
        if input.keys[b'a' as usize] || input.keys[b',' as usize] {
            wish -= right;
        }
        if input.keys[b'd' as usize] || input.keys[b'.' as usize] {
            wish += right;
        }
        // Arrow keys turn when ALT (+strafe) is NOT held
        // Arrow keys strafe when ALT IS held (default.cfg: bind ALT +strafe)
        let strafe_mode = input.keys[K_ALT as usize];
        if input.keys[K_LEFTARROW as usize] {
            if strafe_mode {
                wish -= right;
            } else {
                camera_yaw -= 120.0 * dt; // turn left
            }
        }
        if input.keys[K_RIGHTARROW as usize] {
            if strafe_mode {
                wish += right;
            } else {
                camera_yaw += 120.0 * dt; // turn right
            }
        }

        // Normalize horizontal wish to speed
        let wish_len = (wish.x * wish.x + wish.y * wish.y).sqrt();
        if wish_len > 0.0 {
            wish.x = wish.x / wish_len * speed;
            wish.y = wish.y / wish_len * speed;
        }

        // Keyboard look (default.cfg: bind DEL/z +lookdown, bind PGDN/a_classic +lookup)
        if input.keys[K_DEL as usize] || input.keys[b'z' as usize] {
            camera_pitch += 80.0 * dt;
        }
        if input.keys[K_PGDN as usize] {
            camera_pitch -= 80.0 * dt;
        }
        camera_pitch = camera_pitch.clamp(-89.0, 89.0);

        // Jump (default.cfg: bind SPACE +moveup)
        if input.keys[K_SPACE as usize] && on_ground {
            velocity_z = JUMP_VELOCITY;
            on_ground = false;
        }

        // Crouch (default.cfg: bind c +movedown)
        ducked = input.keys[b'c' as usize];

        // Attack (default.cfg: bind CTRL +attack, bind MOUSE1 +attack)
        let _attacking = input.keys[K_CTRL as usize]
            || input.mouse_buttons & 1 != 0;

        drop(input);

        // Select bbox and viewheight based on crouch state
        // Q2: standing = mins(-16,-16,-24) maxs(16,16,32) viewheight=22
        //     ducked   = mins(-16,-16,-24) maxs(16,16,4)  viewheight=-2
        let player_mins = if ducked { player_mins_duck } else { player_mins_stand };
        let player_maxs = if ducked { player_maxs_duck } else { player_maxs_stand };
        let viewheight: f32 = if ducked { -2.0 } else { 22.0 };

        // Apply gravity
        if !on_ground {
            velocity_z -= GRAVITY * dt;
        }

        // Build desired new position
        let mut new_pos = camera_pos;
        new_pos.x += wish.x;
        new_pos.y += wish.y;
        new_pos.z += velocity_z * dt;

        // Trace horizontal movement (slide against walls)
        {
            let mut cm = collision.borrow_mut();
            let h_target = q2_shared::types::Vec3f::new(new_pos.x, new_pos.y, camera_pos.z);
            let trace = cm.box_trace(camera_pos, h_target, player_mins, player_maxs, 0, MASK_PLAYERSOLID);
            let landed = q2_shared::types::Vec3f::new(
                camera_pos.x + (h_target.x - camera_pos.x) * trace.fraction,
                camera_pos.y + (h_target.y - camera_pos.y) * trace.fraction,
                camera_pos.z,
            );

            // Try step up if blocked
            if trace.fraction < 1.0 {
                let step_start = q2_shared::types::Vec3f::new(landed.x, landed.y, landed.z + 18.0);
                let step_trace = cm.box_trace(
                    q2_shared::types::Vec3f::new(landed.x, landed.y, landed.z),
                    step_start,
                    player_mins, player_maxs, 0, MASK_PLAYERSOLID,
                );
                let step_z = landed.z + (step_start.z - landed.z) * step_trace.fraction;
                let slide_trace = cm.box_trace(
                    q2_shared::types::Vec3f::new(landed.x, landed.y, step_z),
                    q2_shared::types::Vec3f::new(h_target.x, h_target.y, step_z),
                    player_mins, player_maxs, 0, MASK_PLAYERSOLID,
                );
                let stepped = q2_shared::types::Vec3f::new(
                    landed.x + (h_target.x - landed.x) * slide_trace.fraction,
                    landed.y + (h_target.y - landed.y) * slide_trace.fraction,
                    step_z,
                );
                // Step down
                let down_trace = cm.box_trace(
                    stepped,
                    q2_shared::types::Vec3f::new(stepped.x, stepped.y, landed.z),
                    player_mins, player_maxs, 0, MASK_PLAYERSOLID,
                );
                new_pos.x = stepped.x;
                new_pos.y = stepped.y;
                new_pos.z = stepped.z + (landed.z - stepped.z) * down_trace.fraction;
            } else {
                new_pos.x = landed.x;
                new_pos.y = landed.y;
            }

            // Trace vertical movement (gravity / jump)
            let v_start = q2_shared::types::Vec3f::new(new_pos.x, new_pos.y, camera_pos.z);
            let v_end = q2_shared::types::Vec3f::new(new_pos.x, new_pos.y, new_pos.z);
            let v_trace = cm.box_trace(v_start, v_end, player_mins, player_maxs, 0, MASK_PLAYERSOLID);
            new_pos.z = v_start.z + (v_end.z - v_start.z) * v_trace.fraction;

            if v_trace.fraction < 1.0 && velocity_z < 0.0 {
                // Hit ground
                velocity_z = 0.0;
                on_ground = true;
            }

            // Ground check: trace a tiny bit down to detect standing
            let ground_trace = cm.box_trace(
                new_pos,
                q2_shared::types::Vec3f::new(new_pos.x, new_pos.y, new_pos.z - 1.0),
                player_mins, player_maxs, 0, MASK_PLAYERSOLID,
            );
            if ground_trace.fraction < 1.0 {
                on_ground = true;
                if velocity_z < 0.0 { velocity_z = 0.0; }
            } else {
                on_ground = false;
            }
        }

        camera_pos = new_pos;

        // Camera viewpoint is at origin + viewheight
        let view_pos = q2_shared::types::Vec3f::new(
            camera_pos.x, camera_pos.y, camera_pos.z + viewheight,
        );

        // Render frame
        let fd = RefDef {
            x: 0,
            y: 0,
            width,
            height,
            fov_x: 90.0,
            fov_y: q2_client::view::calc_fov(90.0, width as f32, height as f32),
            vieworg: view_pos,
            viewangles: q2_shared::types::Vec3f::new(camera_pitch, camera_yaw, 0.0),
            time: (timestamp / 1000.0) as f32,
            ..Default::default()
        };

        renderer.borrow_mut().render_frame(&fd, &[], &[], &[]);
    }) as Box<dyn FnMut(f64)>;

    q2_platform::wasm::game_loop::start_game_loop(frame_callback)
        .map_err(|e| JsValue::from_str(&e))?;

    Ok(())
}

/// Fetch a URL as raw bytes.
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

/// Player spawn classnames in priority order.
const SPAWN_CLASSNAMES: &[&str] = &[
    "info_player_start",
    "info_player_deathmatch",
    "info_player_coop",
    "info_player_intermission",
    "misc_teleporter_dest",
];

/// Parse BSP entity string to find a player spawn point.
/// Prefers unnamed info_player_start (the default spawn when starting
/// a map fresh). Named spawns (with targetname) are used for level
/// transitions and are lower priority.
fn find_player_start(entities: &str) -> Option<(q2_shared::types::Vec3f, f32)> {
    // Collect all entities with classname + origin + optional targetname
    struct SpawnInfo {
        classname: String,
        origin: q2_shared::types::Vec3f,
        targetname: String,
        angle: f32,
    }
    let mut spawns: Vec<SpawnInfo> = Vec::new();
    let mut classname_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    let mut current_class = String::new();
    let mut current_origin: Option<q2_shared::types::Vec3f> = None;
    let mut current_targetname = String::new();
    let mut current_angle: f32 = 0.0;
    let mut in_entity = false;

    for line in entities.lines() {
        let line = line.trim();
        if line == "{" {
            in_entity = true;
            current_class.clear();
            current_origin = None;
            current_targetname.clear();
            current_angle = 0.0;
        } else if line == "}" {
            if in_entity && !current_class.is_empty() {
                *classname_counts.entry(current_class.clone()).or_insert(0) += 1;
                if let Some(org) = current_origin {
                    spawns.push(SpawnInfo {
                        classname: current_class.clone(),
                        origin: org,
                        targetname: current_targetname.clone(),
                        angle: current_angle,
                    });
                }
            }
            in_entity = false;
        } else if in_entity {
            if let Some((key, val)) = parse_kv(line) {
                if key == "classname" {
                    current_class = val.to_string();
                }
                if key == "origin" {
                    let coords: Vec<f32> = val.split_whitespace()
                        .filter_map(|s| s.parse().ok())
                        .collect();
                    if coords.len() == 3 {
                        current_origin = Some(q2_shared::types::Vec3f::new(coords[0], coords[1], coords[2]));
                    }
                }
                if key == "targetname" {
                    current_targetname = val.to_string();
                }
                if key == "angle" {
                    current_angle = val.parse().unwrap_or(0.0);
                }
            }
        }
    }

    // Log entity summary
    let mut sorted: Vec<_> = classname_counts.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1));
    log("Entity classnames:");
    for (cls, count) in sorted.iter().take(15) {
        log(&format!("  {} x{}", cls, count));
    }

    // Find best spawn point by priority.
    // Prefer unnamed spawns (no targetname) — named ones are for level transitions.
    for target in SPAWN_CLASSNAMES {
        // First try: unnamed spawn of this classname
        if let Some(s) = spawns.iter().find(|s| s.classname == *target && s.targetname.is_empty()) {
            log(&format!("Using spawn: {} at ({:.0}, {:.0}, {:.0}) angle={:.0}",
                target, s.origin.x, s.origin.y, s.origin.z, s.angle));
            return Some((s.origin, s.angle));
        }
        // Second try: any spawn of this classname (even named)
        if let Some(s) = spawns.iter().find(|s| s.classname == *target) {
            log(&format!("Using spawn: {} '{}' at ({:.0}, {:.0}, {:.0}) angle={:.0}",
                target, s.targetname, s.origin.x, s.origin.y, s.origin.z, s.angle));
            return Some((s.origin, s.angle));
        }
    }

    // Last resort: any entity with an origin
    if let Some(s) = spawns.first() {
        log(&format!("Fallback spawn: {} at ({:.0}, {:.0}, {:.0})", s.classname, s.origin.x, s.origin.y, s.origin.z));
        return Some((s.origin, s.angle));
    }

    None
}

/// Parse a "key" "value" line from BSP entity strings.
fn parse_kv(line: &str) -> Option<(&str, &str)> {
    let mut chars = line.char_indices();
    // Find first "
    let start1 = chars.find(|(_, c)| *c == '"')?.0 + 1;
    let end1 = line[start1..].find('"')? + start1;
    // Find second "
    let rest = &line[end1 + 1..];
    let start2 = rest.find('"')? + end1 + 2;
    let end2 = line[start2..].find('"')? + start2;
    Some((&line[start1..end1], &line[start2..end2]))
}

fn log(msg: &str) {
    console::log_1(&format!("[qwasm2-rs] {}", msg).into());
    // Also update the status div if it exists
    if let Some(window) = web_sys::window() {
        if let Some(document) = window.document() {
            if let Some(el) = document.get_element_by_id("status") {
                let current = el.inner_html();
                el.set_inner_html(&format!(
                    "{}<span class=\"info\">{}\n</span>",
                    current, msg
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
