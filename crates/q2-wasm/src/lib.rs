//! Qwasm2-rs WASM entry point.
//!
//! This crate is the `cdylib` target built by `wasm-pack build --target web`.
//! It bootstraps the engine: creates the WebGL2 context, fetches game data,
//! loads the BSP map, and runs the game loop via requestAnimationFrame.
//!
//! # Unsafe Audit Status
//! - Total unsafe blocks: 0
//! - c2rust mechanical: 0
//! - FFI boundary: 0
//! - Performance: 0
//! - Inherent: 0

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
    renderer: Box<dyn Renderer>,
    local: q2_server::local::LocalGame,
    /// The pak filesystem the renderer also reads, for the next map's BSP.
    fs: Rc<q2_common::filesystem::FileSystem>,
    /// Client-side input and view angles (C `cl.viewangles`).
    input: q2_client::input::InputState,
    canvas: web_sys::HtmlCanvasElement,
    view: ViewSize,
    last_time: f64,
    models: ModelTable,
    /// The last two server frames and the client clock (C `cl.frame`,
    /// `cl.time`).
    frames: q2_client::frame::FrameHistory,
    /// Channel and loop choices (C sound.c), and the WebAudio output; no
    /// output means no sound, as when C's `S_Init` fails.
    sound: q2_client::sound::SoundSystem,
    audio: Option<q2_platform::wasm::audio::WebAudio>,
    /// C `vid_gamma`, stepped by F7/F8.
    vid_gamma: f32,
    /// Last frame's keys, so a held F7/F8 steps gamma once.
    last_keys: [bool; 256],
    /// Muzzle flash lights (C `cl_dlights`).
    effects: q2_client::effects::Effects,
    /// Particles, explosions, beams and lasers (C `cl_particles.c`,
    /// `cl_tempentities.c`).
    fx: q2_client::fx::ClientFx,
    /// Each entity's trail state (C `centity_t`).
    trails: q2_client::fx::EntityTrails,
    /// The client's random numbers (C `randk`) for flash sizes and sounds.
    rng: u32,
    /// Printed lines, the centre print and the server layout.
    text: q2_client::notify::ServerText,
    /// C cvar `game`, the game directory (`?game=`; empty is baseq2).
    game: String,
    /// C cvar `hand` (`?hand=`): 0 right, 1 left, 2 centre.
    hand: i32,
    /// C cvar `cl_gun` (`?cl_gun=`): 0 no gun, 1 none in a wide view, 2
    /// always.
    cl_gun: f32,
}

#[cfg(target_arch = "wasm32")]
impl Engine {
    fn new(
        renderer: Box<dyn Renderer>,
        local: q2_server::local::LocalGame,
        fs: Rc<q2_common::filesystem::FileSystem>,
        models: ModelTable,
        canvas: web_sys::HtmlCanvasElement,
        view: ViewSize,
    ) -> Self {
        let mut engine = Self {
            renderer,
            local,
            fs,
            input: q2_client::input::InputState::default(),
            canvas,
            view,
            last_time: q2_platform::wasm::game_loop::performance_now(),
            models,
            frames: q2_client::frame::FrameHistory::default(),
            sound: q2_client::sound::SoundSystem::new(q2_server::local::LOCAL_PLAYER as i32),
            audio: None,
            vid_gamma: 1.0,
            last_keys: [false; 256],
            effects: q2_client::effects::Effects::default(),
            fx: q2_client::fx::ClientFx::default(),
            trails: q2_client::fx::EntityTrails::default(),
            rng: 0x1234_5678,
            text: q2_client::notify::ServerText::default(),
            game: String::new(),
            hand: 0,
            cl_gun: 2.0,
        };
        engine.register_sounds();
        engine
    }

    /// Set `vid_gamma` on the renderer and remember it for the keys.
    fn set_gamma(&mut self, vid_gamma: f32) {
        self.vid_gamma = vid_gamma;
        self.renderer.set_gamma(vid_gamma);
        log(&format!("Gamma: {vid_gamma:.1} (F7 darker, F8 brighter)"));
    }

    /// C cvar `hand`: the renderer mirrors or hides the gun, and heat
    /// beams leave from that side. The game reads it from the userinfo.
    fn set_hand(&mut self, hand: i32) {
        self.hand = hand;
        self.renderer.set_hand(hand);
    }

    fn step_gamma(&mut self, step: i32) {
        let vid_gamma = step_vid_gamma(self.vid_gamma, step);
        if vid_gamma != self.vid_gamma {
            self.set_gamma(vid_gamma);
        }
    }

    /// Run a new server frame's entity events (C `CL_FireEntityEvents`)
    /// and this frame's messages for muzzle flashes, temp entities and
    /// text, then age the lights (C `CL_RunDLights`). Returns the flash
    /// and effect sounds.
    fn run_effects(
        &mut self,
        messages: &[Vec<u8>],
        dt: f32,
        new_frame: bool,
    ) -> (
        Vec<q2_client::effects::FlashSound>,
        Vec<q2_client::fx::FxSound>,
    ) {
        let time_ms = self.frames.time();
        let time = time_ms as f32;
        // C cl.frame.servertime.
        let servertime = self.frames.serverframe().unwrap_or(0).wrapping_mul(100);
        let current = &self.frames.current;
        let entity = |n: i32| current.iter().find(|s| s.number == n).cloned();
        let rng = &mut self.rng;
        let mut randk = || xorshift(rng);
        let mut fx_sounds = Vec::new();
        if new_frame {
            for state in current {
                fx_sounds.extend(self.fx.entity_event(state, time_ms, &mut randk));
            }
        }
        let mut sounds: Vec<q2_client::effects::FlashSound> = messages
            .iter()
            .flat_map(|m| self.effects.message(m, entity, time, &mut randk))
            .collect();
        for message in messages {
            fx_sounds.extend(
                self.fx
                    .temp_entity(message, time_ms, servertime, &mut randk),
            );
        }
        // C allocates these in cl_dlights while it parses the temp entity.
        for light in self.fx.take_dlights() {
            self.effects.add_dlight(light, time);
        }
        for (origin, kind) in self.effects.take_particles() {
            self.fx
                .flash_particles(origin, kind, time_ms, servertime, &mut randk);
        }
        // C con.linewidth: the screen in characters less a margin.
        let linewidth = usize::try_from(self.view.width / 8 - 2).unwrap_or(1);
        for message in messages {
            if self.text.message(message, time as i32, linewidth) {
                sounds.push(talk_sound());
            }
        }
        self.effects.run_dlights(time, dt);
        (sounds, fx_sounds)
    }

    /// C `CL_RegisterSounds`, from the server's `CS_SOUNDS` names.
    fn register_sounds(&mut self) {
        let (local, fs) = (&self.local, &self.fs);
        self.sound
            .register_sounds(|i| local.configstring(i), |path| fs.load_file(path).ok());
    }

    /// Start the sounds the server sent this frame and hand the mixer this
    /// frame's channels and loops, heard from the view (C `S_StartSound`
    /// per `svc_sound`, then `S_Update`). While the browser keeps audio
    /// suspended the sounds are dropped, as C drops them while inactive.
    fn mix_sounds(
        &mut self,
        sounds: &[q2_server::game_iface::SoundEvent],
        flash_sounds: &[q2_client::effects::FlashSound],
        fx_sounds: &[q2_client::fx::FxSound],
        vieworg: q2_shared::types::Vec3f,
        viewangles: q2_shared::types::Vec3f,
        lerpfrac: f32,
    ) {
        use q2_client::sound::{entity_origin, Listener, MixCommand, StartSound};
        use q2_platform::wasm::audio::Pcm;
        let Some(audio) = self.audio.as_mut().filter(|a| a.running()) else {
            return;
        };
        let (local, fs) = (&self.local, &self.fs);
        for ev in sounds {
            let start = StartSound {
                entnum: ev.ent as i32,
                entchannel: ev.channel,
                sound_index: ev.sound_index,
                volume: ev.volume,
                attenuation: ev.attenuation,
                timeofs: ev.time_ofs,
                origin: ev.origin,
            };
            self.sound.start_sound(
                &start,
                |i| local.configstring(i),
                |path| fs.load_file(path).ok(),
            );
        }
        let named = flash_sounds.iter().map(flash_named_sound).chain(
            fx_sounds
                .iter()
                .map(|sound| -> q2_client::sound::NamedSound<'_> { fx_named_sound(sound) }),
        );
        for sound in named {
            self.sound.start_named_sound(
                &sound,
                |i| local.configstring(i),
                |path| fs.load_file(path).ok(),
            );
        }
        let frames = &self.frames;
        let mix = self.sound.update(
            audio.now_ms(),
            &Listener::from_view(vieworg, viewangles),
            |n| entity_origin(&frames.current, &frames.previous, lerpfrac, n),
            &frames.current,
        );
        let sound = &self.sound;
        let pcm = |sfx| {
            sound.sound(sfx).map(|s| Pcm {
                samples: &s.samples,
                rate: s.rate,
                loop_start: s.loop_start,
            })
        };
        for command in mix.commands {
            match command {
                MixCommand::Play {
                    channel,
                    sfx,
                    left,
                    right,
                    delay,
                } => {
                    if let Some(pcm) = pcm(sfx) {
                        audio.play(channel, sfx, pcm, left, right, delay);
                    }
                }
                MixCommand::Gain {
                    channel,
                    left,
                    right,
                } => audio.set_gain(channel, left, right),
                MixCommand::Stop { channel } => audio.stop(channel),
            }
        }
        let loops: Vec<_> = mix
            .loops
            .iter()
            .filter_map(|l| Some((l.sfx, pcm(l.sfx)?, l.left, l.right)))
            .collect();
        audio.set_loops(&loops);
    }

    /// Take the server's entities and player state when a new 10 Hz frame
    /// has run (C CL_ParseFrame keeps the previous one for interpolation),
    /// and keep the renderer's models in step with the model configstrings.
    /// Returns whether a new frame arrived.
    fn sync_frame(&mut self) -> bool {
        let framenum = self.local.framenum();
        if self.frames.serverframe() == Some(framenum) {
            return false;
        }
        let Some((_, ps)) = self.local.player() else {
            return false;
        };
        self.frames
            .receive(framenum, self.local.entity_states(), ps);
        self.trails
            .server_frame(&self.frames.current, &self.frames.previous);
        let local = &self.local;
        let renderer = &mut self.renderer;
        self.models.sync(
            |i| local.configstring(i),
            |name| renderer.register_model(name),
        );
        true
    }

    /// Follow a level exit (C `gamemap`): the server loads the next map
    /// into the same game, then the client preps the renderer for it (C
    /// `CL_PrepRefresh`) and starts from a clear state (C `CL_ClearState`
    /// on the reconnect), so nothing interpolates from the old map.
    fn change_map(&mut self, level: &str) -> q2_common::error::Q2Result<()> {
        use q2_server::init::{LevelKind, MapRequest};
        let request = MapRequest::parse(level);
        log(&format!("Level exit to {level}"));
        // demo2 exits to victory.pcx: a picture level with no map or
        // player, which `tick` draws (C SCR_PlayCinematic). There is no
        // cinematic or demo playback; the current map keeps running.
        match request.kind {
            LevelKind::Game => {}
            LevelKind::Pic => {
                self.local.change_map(&request, &[])?;
                self.frames = q2_client::frame::FrameHistory::default();
                self.effects.clear();
                self.fx.clear();
                self.trails = q2_client::fx::EntityTrails::default();
                self.text.layout.clear();
                log(&format!("Picture: pics/{} shown", request.map));
                return Ok(());
            }
            LevelKind::Cinematic | LevelKind::Demo => {
                log(&format!(
                    "{}: only game maps and pictures can be loaded",
                    request.map
                ));
                return Ok(());
            }
        }
        let bsp = self.fs.load_file(&format!("maps/{}.bsp", request.map))?;
        self.local.change_map(&request, &bsp)?;
        let local = &self.local;
        prep_refresh(
            self.renderer.as_mut(),
            |i| local.configstring(i),
            &mut self.models,
        )?;
        // C CL_ClearState -> S_StopAllSounds, then CL_RegisterSounds.
        self.sound.stop_all();
        self.register_sounds();
        self.frames = q2_client::frame::FrameHistory::default();
        self.effects.clear();
        self.fx.clear();
        self.trails = q2_client::fx::EntityTrails::default();
        self.text.layout.clear();
        self.input.view_angles = q2_shared::types::Vec3f::ZERO;
        log(&format!("Map: maps/{}.bsp loaded", request.map));
        Ok(())
    }

    /// Draw the status bar program over the 3D view (C SCR_DrawStats).
    fn draw_hud(&mut self, stats: &[i16; 32]) {
        use q2_client::layout::{execute_layout, HudDraw, LayoutInput};
        let local = &self.local;
        let configstring = |i: usize| local.configstring(i);
        let input = LayoutInput {
            width: self.view.width,
            height: self.view.height,
            stats,
            configstrings: &configstring,
            serverframe: self.frames.serverframe().unwrap_or(0),
        };
        // C SCR_DrawStats, SCR_DrawLayout, then the centre print and the
        // notify lines.
        let mut draws = execute_layout(
            local.configstring(q2_shared::constants::CS_STATUSBAR),
            &input,
        );
        if stats[q2_shared::constants::STAT_LAYOUTS] & 1 != 0 {
            draws.extend(execute_layout(&self.text.layout, &input));
        }
        draws.extend(
            self.text
                .draws(self.view.width, self.view.height, self.frames.time()),
        );
        for draw in draws {
            match draw {
                HudDraw::Pic { x, y, name } => self.renderer.draw_pic(x, y, &name),
                HudDraw::Char { x, y, ch } => self.renderer.draw_char(x, y, i32::from(ch)),
            }
        }
    }

    /// Run one frame: build a user command from the browser input, apply it
    /// to the local player (C ClientThink), advance the server (game frames
    /// at 10 Hz) and the client clock, and render from the player's eye.
    fn tick(&mut self, timestamp: f64, input: &q2_platform::wasm::input::WasmInputState) {
        let elapsed_us = elapsed_microseconds(timestamp, self.last_time);
        let dt = frame_dt_seconds(timestamp, self.last_time);
        self.last_time = timestamp;

        // C CL_ClampPitch works against the server's delta_angles.
        let delta_pitch = self
            .local
            .player()
            .map(|(_, ps)| f32::from(ps.pmove.delta_angles[0]) * (360.0 / 65536.0))
            .unwrap_or(0.0);
        turn_view(
            &mut self.input,
            &input.keys,
            input.mouse_dx,
            input.mouse_dy,
            dt,
            delta_pitch,
        );
        apply_keys(&input.keys, input.mouse_buttons, &mut self.input);
        self.step_gamma(gamma_key_step(&self.last_keys, &input.keys));
        for command in bound_commands(&self.last_keys, &input.keys) {
            self.local.client_command(command);
        }
        self.last_keys = input.keys;
        let mut cmd = self.input.build_cmd(cmd_msec(dt));
        // From the last rendered frame, as C reads `r_lightlevel`.
        cmd.lightlevel = cmd_lightlevel(self.renderer.light_level());

        // C's func_clock reads localtime: the page's clock.
        let now = js_sys::Date::new_0();
        self.local.set_local_time(Some((
            now.get_hours(),
            now.get_minutes(),
            now.get_seconds(),
        )));
        self.local.think(&cmd);
        // The server and the client clock run on the real elapsed time; only
        // the movement command is capped.
        self.local.frame(elapsed_us);
        let sounds = self.local.take_sounds();
        let messages = self.local.take_messages();
        if let Some(level) = self.local.take_map_change() {
            if let Err(e) = self.change_map(&level) {
                console::error_1(&q2err_to_js(e));
            }
        }
        self.frames.advance(elapsed_us);
        let new_frame = self.sync_frame();
        let (flash_sounds, fx_sounds) = self.run_effects(&messages, dt, new_frame);

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

        // A key press would send C's `nextserver`, which SV_Nextserver
        // ignores for a picture outside coop: the picture stays.
        if let Some(name) = self.local.picture() {
            picture_frame(
                self.renderer.as_mut(),
                self.view.width,
                self.view.height,
                name,
            );
            return;
        }

        let lerpfrac = self.frames.lerpfrac();
        let cl_time = self.frames.time();
        let Some((origin, ps)) = self.local.player() else {
            return;
        };
        let (vieworg, viewangles) = self.frames.view(origin, ps, lerpfrac);
        let (stats, blend, rdflags) = (ps.stats, ps.blend, ps.rdflags);
        self.mix_sounds(
            &sounds,
            &flash_sounds,
            &fx_sounds,
            vieworg,
            viewangles,
            lerpfrac,
        );
        let fd = RefDef {
            x: 0,
            y: 0,
            width: self.view.width,
            height: self.view.height,
            fov_x: FOV_X,
            fov_y: self.view.fov_y,
            vieworg,
            viewangles,
            // C cl.refdef.time = cl.time * 0.001; light styles step on cl.time.
            time: cl_time as f32 * 0.001,
            lightstyles: q2_client::lightstyles::style_values(
                |i| self.local.configstring(q2_shared::constants::CS_LIGHTS + i),
                u32::try_from(cl_time).unwrap_or(0),
            ),
            // C cl.refdef.areabits = cl.frame.areabits: shut area portals
            // hide the world behind them.
            areabits: self.local.area_bits(),
            // C CL_CalcViewValues: the server's tint (powerups, liquids).
            blend,
            rdflags,
        };

        let mut views = q2_client::entities::entity_views(
            &self.game,
            &self.frames.current,
            &self.frames.previous,
            lerpfrac,
            cl_time,
            (q2_server::local::LOCAL_PLAYER - 1) as i32,
        );
        let rng = &mut self.rng;
        q2_client::entities::pick_beam_colours(&mut views, &mut || xorshift(rng));
        let mut entities = render_entities(&views, &self.models);
        let gun = self
            .frames
            .view_weapon(vieworg, viewangles, lerpfrac, self.cl_gun);
        entities.extend(gun.and_then(|gun| gun_entity(&gun, &self.models)));
        // C CL_AddPacketEntities' trails, then CL_AddTEnts and
        // CL_AddParticles; C cl.playernum + 1 is the local player.
        let player = q2_server::local::LOCAL_PLAYER as i32;
        let rng = &mut self.rng;
        let mut randk = || xorshift(rng);
        let mut lights = self.trails.run(
            &mut self.fx,
            &self.frames.current,
            lerpfrac,
            player,
            cl_time,
            &mut randk,
        );
        let view = q2_client::fx::FxView::new(
            player,
            vieworg,
            viewangles,
            self.frames.gun_offsets(),
            self.hand,
        );
        let mut effects = self.fx.frame_view(cl_time, lerpfrac, &view, &mut randk);
        effects.entities.extend(power_screens(&views));
        entities.extend(fx_render_entities(&effects.entities, &self.models));
        lights.extend(effects.lights);
        self.renderer.begin_frame(self.view.width, self.view.height);
        let mut dlights = render_dlights(&self.effects);
        dlights.extend(fx_dlights(&lights));
        self.renderer
            .render_frame(&fd, &entities, &dlights, &fx_particles(&effects.particles));
        self.draw_hud(&stats);
        self.renderer.end_frame();
    }
}

/// Microseconds since the last frame for the server and client clocks (C
/// measures the frame in whole microseconds), rounded so float error in the
/// millisecond timestamps does not drop a microsecond every frame.
/// Negative deltas count as 0.
#[cfg(any(target_arch = "wasm32", test))]
fn elapsed_microseconds(timestamp_ms: f64, last_ms: f64) -> u32 {
    ((timestamp_ms - last_ms) * 1000.0)
        .round()
        .clamp(0.0, f64::from(u32::MAX)) as u32
}

/// Seconds since the last frame for the movement tick and view turning,
/// clamped to `0.0..=0.5` like C `cls.nframetime` ("don't extrapolate too
/// far ahead"). Negative deltas (the first RAF timestamp can precede the
/// `performance.now()` taken at engine creation) would run movement and
/// gravity backwards for a frame.
#[cfg(any(target_arch = "wasm32", test))]
fn frame_dt_seconds(timestamp_ms: f64, last_ms: f64) -> f32 {
    ((timestamp_ms - last_ms) / 1000.0).clamp(0.0, 0.5) as f32
}

/// The renderer's handle for each `CS_MODELS` index (C `cl.model_draw`),
/// re-registered when the server changes a model configstring.
#[cfg(any(target_arch = "wasm32", test))]
#[derive(Default)]
struct ModelTable {
    names: Vec<String>,
    handles: Vec<q2_render_api::ModelHandle>,
    /// The effects' models (C `cl_mod_explode` and the rest).
    fx: Vec<(&'static str, q2_render_api::ModelHandle)>,
}

#[cfg(any(target_arch = "wasm32", test))]
impl ModelTable {
    /// Register every changed name from `configstring(CS_MODELS + i)`.
    /// Index 1 is the world map, which the renderer already has.
    fn sync<'a>(
        &mut self,
        configstring: impl Fn(usize) -> &'a str,
        mut register: impl FnMut(&str) -> q2_render_api::ModelHandle,
    ) {
        use q2_shared::constants::{CS_MODELS, MAX_MODELS};
        self.names.resize(MAX_MODELS, String::new());
        self.handles
            .resize(MAX_MODELS, q2_render_api::ModelHandle::NONE);
        for i in 2..MAX_MODELS {
            let name = configstring(CS_MODELS + i);
            if self.names[i] != name {
                self.names[i] = name.to_string();
                self.handles[i] = if name.is_empty() {
                    q2_render_api::ModelHandle::NONE
                } else {
                    register(name)
                };
            }
        }
    }

    /// C `CL_RegisterTEntModels`.
    fn register_fx(&mut self, mut register: impl FnMut(&str) -> q2_render_api::ModelHandle) {
        self.fx = q2_client::fx::FX_MODELS
            .iter()
            .map(|&name| (name, register(name)))
            .collect();
    }

    fn fx_handle(&self, name: &str) -> q2_render_api::ModelHandle {
        self.fx
            .iter()
            .find(|(n, _)| *n == name)
            .map_or(q2_render_api::ModelHandle::NONE, |&(_, handle)| handle)
    }

    fn handle(&self, modelindex: i32) -> q2_render_api::ModelHandle {
        usize::try_from(modelindex)
            .ok()
            .and_then(|i| self.handles.get(i).copied())
            .unwrap_or(q2_render_api::ModelHandle::NONE)
    }
}

/// Load the server's map into the renderer and register what its
/// configstrings name (C `CL_PrepRefresh`): `begin_registration` with the
/// world from `CS_MODELS + 1`, every model (into a fresh `models`, so a
/// model the old map also used is registered again and survives the
/// sweep), the images up to the first empty one, the sky, then
/// `end_registration` to free what the new map does not use. With no world
/// model configstring there is nothing to load yet, as in C.
#[cfg(any(target_arch = "wasm32", test))]
fn prep_refresh<'a>(
    renderer: &mut dyn q2_render_api::Renderer,
    configstring: impl Fn(usize) -> &'a str,
    models: &mut ModelTable,
) -> q2_common::error::Q2Result<()> {
    use q2_shared::constants::{
        CS_IMAGES, CS_MODELS, CS_SKY, CS_SKYAXIS, CS_SKYROTATE, MAX_IMAGES,
    };
    let world = configstring(CS_MODELS + 1);
    if world.is_empty() {
        return Ok(());
    }
    // C skips "maps/" and cuts ".bsp".
    let map = world.strip_prefix("maps/").unwrap_or(world);
    renderer.begin_registration(map.strip_suffix(".bsp").unwrap_or(map))?;
    *models = ModelTable::default();
    models.register_fx(|name| renderer.register_model(name));
    models.sync(&configstring, |name| renderer.register_model(name));
    // C keeps these in `cl.image_precache`, which nothing draws from: the
    // status bar draws pics by name.
    for i in 1..MAX_IMAGES {
        let name = configstring(CS_IMAGES + i);
        if name.is_empty() {
            break;
        }
        renderer.register_image(name, q2_render_api::ImageType::Pic);
    }
    let rotate = q2_common::cvar::parse_value(configstring(CS_SKYROTATE));
    renderer.set_sky(
        configstring(CS_SKY),
        rotate,
        sky_axis(configstring(CS_SKYAXIS)),
    );
    renderer.end_registration();
    Ok(())
}

/// One frame of a picture level (C `SCR_UpdateScreen` with a cinematic
/// up): `begin_frame` sets 2D up for the canvas, which may have been
/// resized, since no `render_frame` runs to do it.
#[cfg(any(target_arch = "wasm32", test))]
fn picture_frame(renderer: &mut dyn q2_render_api::Renderer, width: i32, height: i32, name: &str) {
    renderer.begin_frame(width, height);
    draw_picture(renderer, width, height, name);
    renderer.end_frame();
}

/// Draw a picture level's `pics/<name>` in place of the view and status
/// bar (C `SCR_DrawCinematic` with its default `cin_force43 1`): stretched
/// to the largest centred 4:3 rectangle, width a multiple of 4, with
/// palette colour 0 filling the rest. C draws the decoded PCX with its own
/// palette (`Draw_StretchRaw`); `victory.pcx` differs from the colormap
/// only in indices 48-51 and 255, none of which it uses, so the renderer's
/// colormap pic draws it the same.
#[cfg(any(target_arch = "wasm32", test))]
fn draw_picture(renderer: &mut dyn q2_render_api::Renderer, width: i32, height: i32, name: &str) {
    let w = ((height * 4 / 3).min(width)) & !3;
    let h = w * 3 / 4;
    let (x, y) = ((width - w) / 2, (height - h) / 2);
    if x > 0 {
        renderer.draw_fill(0, 0, x, height, 0);
    }
    if x + w < width {
        renderer.draw_fill(x + w, 0, width - (x + w), height, 0);
    }
    if y > 0 {
        renderer.draw_fill(x, 0, w, y, 0);
    }
    if y + h < height {
        renderer.draw_fill(x, y + h, w, height - (y + h), 0);
    }
    renderer.draw_stretch_pic(x, y, w, h, &format!("pics/{name}"));
}

/// `CS_SKYAXIS` as C's `sscanf("%f %f %f")`; missing components read as 0.
#[cfg(any(target_arch = "wasm32", test))]
fn sky_axis(text: &str) -> q2_shared::types::Vec3f {
    let mut axis = text.split_whitespace().map(q2_common::cvar::parse_value);
    let mut next = || axis.next().unwrap_or(0.0);
    q2_shared::types::Vec3f::new(next(), next(), next())
}

/// The renderer's entity list for `views`; models it could not register
/// are left out, except beams, which have none.
#[cfg(any(target_arch = "wasm32", test))]
fn render_entities(
    views: &[q2_client::entities::EntityView],
    models: &ModelTable,
) -> Vec<q2_render_api::RenderEntity> {
    views
        .iter()
        .filter_map(|v| {
            let model = models.handle(v.modelindex);
            let drawable = model != q2_render_api::ModelHandle::NONE
                || v.flags & q2_shared::constants::RF_BEAM != 0;
            drawable.then(|| q2_render_api::RenderEntity {
                model,
                frame: v.frame,
                oldframe: v.oldframe,
                backlerp: v.backlerp,
                origin: v.origin,
                oldorigin: v.oldorigin,
                angles: v.angles,
                alpha: v.alpha,
                flags: v.flags,
                skinnum: v.skinnum,
                ..Default::default()
            })
        })
        .collect()
}

/// C `S_StartLocalSound("misc/talk.wav")` for a chat line.
#[cfg(any(target_arch = "wasm32", test))]
fn talk_sound() -> q2_client::effects::FlashSound {
    q2_client::effects::FlashSound {
        entnum: q2_server::local::LOCAL_PLAYER as i32,
        channel: 0,
        name: "misc/talk.wav".into(),
        volume: 1.0,
        attenuation: 0.0,
        timeofs: 0.0,
    }
}

/// The lit dynamic lights for the renderer (C `V_AddLight`: the radius
/// is the intensity).
#[cfg(any(target_arch = "wasm32", test))]
fn render_dlights(effects: &q2_client::effects::Effects) -> Vec<q2_render_api::DLight> {
    effects
        .dlights()
        .map(|light| q2_render_api::DLight {
            origin: light.origin,
            color: q2_shared::types::Vec3f::from_array(light.color),
            intensity: light.radius,
        })
        .collect()
}

/// The effects' models for the renderer (C `V_AddEntity` in
/// `CL_AddTEnts`): a beam needs no model, the rest are left out when
/// theirs did not register.
#[cfg(any(target_arch = "wasm32", test))]
fn fx_render_entities(
    entities: &[q2_client::fx::FxEntity],
    models: &ModelTable,
) -> Vec<q2_render_api::RenderEntity> {
    entities
        .iter()
        .filter_map(|e| {
            let model = e.model.map_or(q2_render_api::ModelHandle::NONE, |name| {
                models.fx_handle(name)
            });
            let drawable = e.flags & q2_shared::constants::RF_BEAM != 0
                || model != q2_render_api::ModelHandle::NONE;
            drawable.then(|| q2_render_api::RenderEntity {
                model,
                frame: e.frame,
                oldframe: e.oldframe,
                backlerp: e.backlerp,
                origin: e.origin,
                oldorigin: e.oldorigin,
                angles: e.angles,
                alpha: e.alpha,
                flags: e.flags,
                skinnum: e.skinnum,
                ..Default::default()
            })
        })
        .collect()
}

/// C `CL_AddPacketEntities`' `EF_POWERSCREEN` model over every entity
/// but the viewer, whom C skips before it.
#[cfg(any(target_arch = "wasm32", test))]
fn power_screens(views: &[q2_client::entities::EntityView]) -> Vec<q2_client::fx::FxEntity> {
    views
        .iter()
        .filter(|v| {
            v.effects & q2_shared::constants::EF_POWERSCREEN != 0
                && v.flags & q2_shared::constants::RF_VIEWERMODEL == 0
        })
        .map(q2_client::fx::powerscreen_entity)
        .collect()
}

/// C `V_AddParticle` for each live particle.
#[cfg(any(target_arch = "wasm32", test))]
fn fx_particles(particles: &[q2_client::particles::DrawParticle]) -> Vec<q2_render_api::Particle> {
    particles
        .iter()
        .map(|p| q2_render_api::Particle {
            origin: p.origin,
            color: i32::from(p.color),
            alpha: p.alpha,
        })
        .collect()
}

/// C `V_AddLight` for the effects' lights.
#[cfg(any(target_arch = "wasm32", test))]
fn fx_dlights(lights: &[q2_client::fx::FxLight]) -> Vec<q2_render_api::DLight> {
    lights
        .iter()
        .map(|l| q2_render_api::DLight {
            origin: l.origin,
            color: q2_shared::types::Vec3f::from_array(l.color),
            intensity: l.intensity,
        })
        .collect()
}

#[cfg(any(target_arch = "wasm32", test))]
fn fx_named_sound(sound: &q2_client::fx::FxSound) -> q2_client::sound::NamedSound<'static> {
    q2_client::sound::NamedSound {
        name: sound.name,
        entnum: sound.entnum,
        entchannel: sound.channel,
        volume: sound.volume,
        attenuation: sound.attenuation,
        timeofs: sound.timeofs,
        origin: sound.origin,
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn flash_named_sound(sound: &q2_client::effects::FlashSound) -> q2_client::sound::NamedSound<'_> {
    q2_client::sound::NamedSound {
        name: &sound.name,
        entnum: sound.entnum,
        entchannel: sound.channel,
        volume: sound.volume,
        attenuation: sound.attenuation,
        timeofs: sound.timeofs,
        origin: None,
    }
}

/// A xorshift32 step: the client's `randk`, 31 bits like C's. C's
/// generator (KISS) differs, so the effects' random spreads do too.
#[cfg(any(target_arch = "wasm32", test))]
fn xorshift(state: &mut u32) -> u32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    // C randk is never negative (it takes the absolute value).
    x & 0x7fff_ffff
}

/// The first-person gun as a render entity (C `CL_AddViewWeapon`'s
/// `V_AddEntity`), if its model is registered.
#[cfg(any(target_arch = "wasm32", test))]
fn gun_entity(
    gun: &q2_client::frame::ViewWeapon,
    models: &ModelTable,
) -> Option<q2_render_api::RenderEntity> {
    use q2_shared::constants::{RF_DEPTHHACK, RF_MINLIGHT, RF_WEAPONMODEL};
    let model = models.handle(gun.modelindex);
    (model != q2_render_api::ModelHandle::NONE).then(|| q2_render_api::RenderEntity {
        model,
        frame: gun.frame,
        oldframe: gun.oldframe,
        backlerp: gun.backlerp,
        origin: gun.origin,
        oldorigin: gun.origin,
        angles: gun.angles,
        flags: RF_MINLIGHT | RF_DEPTHHACK | RF_WEAPONMODEL,
        ..Default::default()
    })
}

/// Mouse sensitivity (degrees per pixel of mouse movement). A choice of
/// this port, not C's default (`sensitivity 3` times `m_yaw 0.022` is
/// 0.066 deg/px); there is no sensitivity cvar yet.
#[cfg(any(target_arch = "wasm32", test))]
const MOUSE_SENSITIVITY: f32 = 0.15;

/// C `cl_yawspeed` / `cl_pitchspeed` defaults: keyboard turning, deg/s.
#[cfg(any(target_arch = "wasm32", test))]
const CL_YAWSPEED: f32 = 140.0;
#[cfg(any(target_arch = "wasm32", test))]
const CL_PITCHSPEED: f32 = 150.0;
/// C `cl_anglespeedkey` default: the turn speed factor while +speed is held.
#[cfg(any(target_arch = "wasm32", test))]
const CL_ANGLESPEEDKEY: f32 = 1.5;

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

/// The light level a user command carries (C CL_FinishMove's
/// `(byte)cl_lightlevel->value`): truncated, keeping the low 8 bits as the
/// C conversion does on wasm and x86.
#[cfg(any(target_arch = "wasm32", test))]
fn cmd_lightlevel(r_lightlevel: f32) -> u8 {
    r_lightlevel as i32 as u8
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

/// Command keys from Q2's default.cfg: weapons on the number row and the
/// inventory keys. Each fires on the frame the key goes down (C key binds
/// run on the down event).
#[cfg(any(target_arch = "wasm32", test))]
const BINDS: [(u8, &str); 15] = [
    (q2_platform::keymap::K_F1, "help"),
    (b'1', "use Blaster"),
    (b'2', "use Shotgun"),
    (b'3', "use Super Shotgun"),
    (b'4', "use Machinegun"),
    (b'5', "use Chaingun"),
    (b'6', "use Grenade Launcher"),
    (b'7', "use Rocket Launcher"),
    (b'8', "use HyperBlaster"),
    (b'9', "use Railgun"),
    (b'0', "use BFG10K"),
    (b'[', "invprev"),
    (b']', "invnext"),
    (b'\'', "invdrop"),
    (q2_platform::keymap::K_ENTER, "invuse"),
];

/// The bound commands whose keys went down since `last_keys`, in key
/// order.
#[cfg(any(target_arch = "wasm32", test))]
fn bound_commands(last_keys: &[bool; 256], keys: &[bool; 256]) -> Vec<&'static str> {
    let mut commands: Vec<(u8, &str)> = BINDS
        .iter()
        .filter(|&&(k, _)| keys[k as usize] && !last_keys[k as usize])
        .copied()
        .collect();
    commands.sort_by_key(|&(k, _)| k);
    commands.into_iter().map(|(_, command)| command).collect()
}

/// Range of C's video menu brightness slider (`s_brightness_slider`), which
/// is the only way C sets `vid_gamma` short of the console.
#[cfg(any(target_arch = "wasm32", test))]
const VID_GAMMA_MIN: f32 = 0.1;
#[cfg(any(target_arch = "wasm32", test))]
const VID_GAMMA_MAX: f32 = 2.0;

/// The value of `key` in the page's query string (`?map=demo3&hand=1`).
#[cfg(any(target_arch = "wasm32", test))]
fn query_value<'a>(search: &'a str, key: &str) -> Option<&'a str> {
    search
        .trim_start_matches('?')
        .split('&')
        .find_map(|pair| pair.strip_prefix(key)?.strip_prefix('='))
}

/// The map to start on: `?map=` (`demo3` or `maps/demo3.bsp`) when the pak
/// has it, else demo1, else the first map.
#[cfg(any(target_arch = "wasm32", test))]
fn start_map<'a>(requested: Option<&str>, maps: &[&'a str]) -> Option<&'a str> {
    let wanted = requested.map(|name| {
        let name = name.strip_prefix("maps/").unwrap_or(name);
        format!("maps/{}.bsp", name.strip_suffix(".bsp").unwrap_or(name))
    });
    wanted
        .and_then(|wanted| maps.iter().find(|&&m| m == wanted))
        .or_else(|| maps.iter().find(|&&m| m == "maps/demo1.bsp"))
        .or_else(|| maps.first())
        .copied()
}

/// C cvar `hand` (0 right, 1 left, 2 centre) from `?hand=`; 0 when unset
/// or not a number.
#[cfg(any(target_arch = "wasm32", test))]
fn hand_from_query(search: &str) -> i32 {
    query_value(search, "hand")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

/// The local client's userinfo: C sends its `CVAR_USERINFO` cvars, of
/// which this port has only `hand`.
#[cfg(any(target_arch = "wasm32", test))]
fn local_userinfo(hand: i32) -> String {
    format!("\\hand\\{hand}")
}

/// C cvar `cl_gun` from `?cl_gun=`, as text for the game; C's default 2.
#[cfg(any(target_arch = "wasm32", test))]
fn cl_gun_from_query(search: &str) -> &str {
    query_value(search, "cl_gun").unwrap_or("2")
}

/// C cvar `vid_fullscreen` from `?vid_fullscreen=1`: clicking to play
/// also goes fullscreen with the keyboard locked to the game.
#[cfg(any(target_arch = "wasm32", test))]
fn vid_fullscreen_from_query(search: &str) -> bool {
    query_value(search, "vid_fullscreen") == Some("1")
}

/// `?cl_fxextra=1`: effects C does not draw (ADR-003), off by default.
#[cfg(any(target_arch = "wasm32", test))]
fn fx_extras_from_query(search: &str) -> bool {
    query_value(search, "cl_fxextra") == Some("1")
}

/// C cvar `game` (the game directory) from `?game=`; empty is baseq2.
#[cfg(any(target_arch = "wasm32", test))]
fn game_from_query(search: &str) -> &str {
    query_value(search, "game").unwrap_or("")
}

/// `vid_gamma` from the page's query string (`?gamma=1.2`), clamped to the
/// slider range; `None` keeps the default.
#[cfg(any(target_arch = "wasm32", test))]
fn vid_gamma_from_query(search: &str) -> Option<f32> {
    let value = query_value(search, "gamma")?;
    let vid_gamma = value.parse::<f32>().ok().filter(|v| v.is_finite())?;
    Some(vid_gamma.clamp(VID_GAMMA_MIN, VID_GAMMA_MAX))
}

/// Move `vid_gamma` by `step` tenths (the slider's step), on the tenths grid
/// so repeated presses don't drift.
#[cfg(any(target_arch = "wasm32", test))]
fn step_vid_gamma(vid_gamma: f32, step: i32) -> f32 {
    let tenths = (vid_gamma * 10.0).round() + step as f32;
    (tenths / 10.0).clamp(VID_GAMMA_MIN, VID_GAMMA_MAX)
}

/// +1 when F8 (brighter) went down this frame, -1 for F7 (darker). Q2's
/// default.cfg leaves both unbound; `[`/`]` are invprev/invnext.
#[cfg(any(target_arch = "wasm32", test))]
fn gamma_key_step(last_keys: &[bool; 256], keys: &[bool; 256]) -> i32 {
    use q2_platform::keymap::{K_F7, K_F8};
    let pressed = |k: u8| i32::from(keys[k as usize] && !last_keys[k as usize]);
    pressed(K_F8) - pressed(K_F7)
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
    // +speed (shift) turns faster too.
    let speed = if down(K_SHIFT) {
        dt * CL_ANGLESPEEDKEY
    } else {
        dt
    };
    if !down(K_ALT) {
        if down(K_LEFTARROW) {
            angles.y += CL_YAWSPEED * speed;
        }
        if down(K_RIGHTARROW) {
            angles.y -= CL_YAWSPEED * speed;
        }
    }
    if down(K_PGDN) {
        angles.x -= CL_PITCHSPEED * speed; // +lookup
    }
    if down(K_DEL) {
        angles.x += CL_PITCHSPEED * speed; // +lookdown
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
    log("WebGL2 context created");

    let view = ViewSize::for_canvas(canvas.width(), canvas.height())
        .map_err(q2err_to_js)?
        .ok_or_else(|| JsValue::from_str("canvas has no usable size"))?;
    let (width, height) = (view.width, view.height);
    log(&format!("Canvas: {}x{}", width, height));

    // 2. Fetch pak0.pak — keep bytes in JS heap; don't copy to Rust yet.
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

    // 6. Load the first map's BSP: `?map=`, else demo1.
    let search = web_sys::window()
        .and_then(|w| w.location().search().ok())
        .unwrap_or_default();
    let requested = query_value(&search, "map");
    let Some(map_name) = start_map(requested, &map_list) else {
        return Err(JsValue::from_str("No maps found in pak0.pak"));
    };
    if let Some(requested) = requested.filter(|r| !map_name.contains(*r)) {
        log(&format!("?map={requested} is not in the pak"));
    }

    log(&format!("Loading {}...", map_name));
    let bsp_data = fs.load_file(map_name).map_err(q2err_to_js)?;
    log(&format!("BSP loaded: {} bytes", bsp_data.len()));

    // 7. Renderer (C R_Init): it reads the world, textures, sky and palette
    //    through the same filesystem.
    let short_name = map_name
        .trim_start_matches("maps/")
        .trim_end_matches(".bsp");
    let fs = Rc::new(fs);
    let loader_fs = fs.clone();
    let mut renderer: Box<dyn Renderer> = Box::new(Gl3Renderer::new(
        gl,
        Box::new(move |path| loader_fs.load_file(path).ok()),
    ));
    renderer.init(width, height).map_err(q2err_to_js)?;
    log("GL3 renderer initialized");

    // 8. Server and game: load collision, spawn the map's entities, and put
    //    the local player in at the player start.
    let hand = hand_from_query(&search);
    let local = q2_server::local::LocalGame::start_with_userinfo(
        short_name,
        &bsp_data,
        &local_userinfo(hand),
    )
    .map_err(q2err_to_js)?;
    log(&format!("Game: {} entities spawned", local.num_edicts()));
    if let Some((origin, ps)) = local.player() {
        log(&format!(
            "Player start: ({:.0}, {:.0}, {:.0}) yaw={:.0}",
            origin.x, origin.y, origin.z, ps.viewangles.y
        ));
    }

    // C CL_PrepRefresh once the server's configstrings are in.
    let mut models = ModelTable::default();
    prep_refresh(renderer.as_mut(), |i| local.configstring(i), &mut models).map_err(q2err_to_js)?;
    log("World uploaded to GPU");

    let mut engine = Engine::new(renderer, local, fs, models, canvas.clone(), view);
    engine.set_gamma(vid_gamma_from_query(&search).unwrap_or(1.0));
    engine.set_hand(hand);
    engine.game = game_from_query(&search).to_string();
    engine.fx.extras = fx_extras_from_query(&search);
    let cl_gun = cl_gun_from_query(&search);
    engine.cl_gun = q2_common::cvar::parse_value(cl_gun);
    engine.local.set_cvar("cl_gun", cl_gun);

    // 9. Input, last of the fallible steps before the loop: the listeners
    //    are leaked for the page's lifetime, so a failed start that is
    //    retried must not have attached them already.
    let input_state = q2_platform::wasm::input::setup_input_listeners(
        &canvas,
        vid_fullscreen_from_query(&search),
    )
    .map_err(q2err_to_js)?;
    log("Input listeners attached (click canvas for pointer lock)");

    // Sound plays once the browser lets the context run (a click).
    match q2_platform::wasm::audio::WebAudio::new(
        &canvas,
        Box::new(|state| log(&format!("Audio: context {state}"))),
    ) {
        Ok(audio) => engine.audio = Some(audio),
        Err(e) => console::warn_1(&q2err_to_js(e)),
    }

    // 10. Start game loop
    log("Starting game loop...");
    log(&format!("Map: {} — engine running", map_name));

    let engine = Rc::new(RefCell::new(engine));
    // No catch_unwind: wasm32-unknown-unknown aborts on panic, so a frame
    // panic traps the module; the panic hook (wasm_main) logs it. The input
    // state is copied out under a short borrow so a trap in `tick` does not
    // leave it borrowed for the listeners, which would then panic on every
    // event and bury the first panic.
    let frame_callback = Box::new(move |timestamp: f64| {
        let input = {
            let mut shared = input_state.borrow_mut();
            let input = shared.clone();
            shared.mouse_dx = 0.0;
            shared.mouse_dy = 0.0;
            input
        };
        engine.borrow_mut().tick(timestamp, &input);
    }) as Box<dyn FnMut(f64)>;

    q2_platform::wasm::game_loop::start_game_loop(
        frame_callback,
        Box::new(|msg| log(&format!("Game loop stopped: {msg}"))),
    )
    .map_err(q2err_to_js)?;

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
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let Some(status) = document.get_element_by_id("status") else {
        return;
    };
    // Append a span rather than rewriting the element's HTML each call.
    let line = match document.create_element("span") {
        Ok(line) => line,
        Err(e) => return console::error_1(&e),
    };
    line.set_class_name("info");
    line.set_text_content(Some(&format!("{msg}\n")));
    if let Err(e) = status.append_child(&line) {
        console::error_1(&e);
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

    /// A renderer that records the registration calls `prep_refresh` makes.
    #[derive(Default)]
    struct Recorder {
        calls: Vec<String>,
        fail_map: bool,
    }

    impl q2_render_api::Renderer for Recorder {
        fn init(&mut self, _: i32, _: i32) -> q2_common::error::Q2Result<()> {
            Ok(())
        }
        fn shutdown(&mut self) {}
        fn begin_registration(&mut self, map: &str) -> q2_common::error::Q2Result<()> {
            self.calls.push(format!("begin {map}"));
            if self.fail_map {
                return Err(q2_common::Q2Error::Drop(format!("{map}: missing")));
            }
            Ok(())
        }
        fn register_model(&mut self, name: &str) -> q2_render_api::ModelHandle {
            self.calls.push(format!("model {name}"));
            q2_render_api::ModelHandle::new(self.calls.len() as i32)
        }
        fn register_image(
            &mut self,
            name: &str,
            kind: q2_render_api::ImageType,
        ) -> q2_render_api::ImageHandle {
            self.calls.push(format!("image {name} {kind:?}"));
            q2_render_api::ImageHandle::NONE
        }
        fn end_registration(&mut self) {
            self.calls.push("end".into());
        }
        fn render_frame(
            &mut self,
            _: &q2_render_api::RefDef,
            _: &[q2_render_api::RenderEntity],
            _: &[q2_render_api::DLight],
            _: &[q2_render_api::Particle],
        ) {
        }
        fn set_sky(&mut self, name: &str, rotate: f32, axis: q2_shared::types::Vec3f) {
            self.calls.push(format!(
                "sky {name} {rotate} {} {} {}",
                axis.x, axis.y, axis.z
            ));
        }
        fn draw_pic(&mut self, _: i32, _: i32, _: &str) {}
        fn draw_get_pic_size(&mut self, _: &str) -> Option<(i32, i32)> {
            None
        }
        fn set_gamma(&mut self, vid_gamma: f32) {
            self.calls.push(format!("gamma {vid_gamma}"));
        }
        fn set_hand(&mut self, hand: i32) {
            self.calls.push(format!("hand {hand}"));
        }
        fn draw_stretch_pic(&mut self, x: i32, y: i32, w: i32, h: i32, name: &str) {
            self.calls.push(format!("stretch {x} {y} {w} {h} {name}"));
        }
        fn draw_char(&mut self, _: i32, _: i32, _: i32) {}
        fn draw_fill(&mut self, x: i32, y: i32, w: i32, h: i32, color: i32) {
            self.calls.push(format!("fill {x} {y} {w} {h} {color}"));
        }
        fn clear_screen(&mut self) {}
        fn begin_frame(&mut self, width: i32, height: i32) {
            self.calls.push(format!("begin_frame {width} {height}"));
        }
        fn end_frame(&mut self) {
            self.calls.push("end_frame".into());
        }
    }

    fn demo2_strings(i: usize) -> &'static str {
        use q2_shared::constants::*;
        match i {
            _ if i == CS_MODELS + 1 => "maps/demo2.bsp",
            _ if i == CS_MODELS + 2 => "*1",
            _ if i == CS_MODELS + 3 => "models/items/armor/jacket/tris.md2",
            _ if i == CS_IMAGES + 1 => "i_health",
            _ if i == CS_IMAGES + 2 => "a_blaster",
            _ if i == CS_IMAGES + 4 => "after_a_gap",
            _ if i == CS_SKY => "unit2_",
            _ if i == CS_SKYROTATE => "2.5",
            _ if i == CS_SKYAXIS => "0 0 1",
            _ => "",
        }
    }

    #[test]
    fn prep_refresh_registers_the_new_map_in_cl_prep_refresh_order() {
        let mut renderer = Recorder::default();
        let mut models = ModelTable::default();
        prep_refresh(&mut renderer, demo2_strings, &mut models).unwrap();
        assert_eq!(
            renderer.calls,
            [
                "begin demo2",
                // C CL_RegisterTEntModels, before the configstring models.
                "model models/objects/explode/tris.md2",
                "model models/objects/smoke/tris.md2",
                "model models/objects/flash/tris.md2",
                "model models/monsters/parasite/segment/tris.md2",
                "model models/ctf/segment/tris.md2",
                "model models/monsters/parasite/tip/tris.md2",
                "model models/objects/r_explode/tris.md2",
                "model sprites/s_bfg2.sp2",
                "model models/items/armor/effect/tris.md2",
                "model models/objects/r_explode2/tris.md2",
                "model models/proj/lightning/tris.md2",
                "model models/proj/beam/tris.md2",
                "model models/proj/widowbeam/tris.md2",
                "model *1",
                "model models/items/armor/jacket/tris.md2",
                // C stops at the first empty image configstring.
                "image i_health Pic",
                "image a_blaster Pic",
                "sky unit2_ 2.5 0 0 1",
                "end",
            ]
        );
        assert_ne!(models.handle(3), q2_render_api::ModelHandle::NONE);
        assert_ne!(
            models.fx_handle("sprites/s_bfg2.sp2"),
            q2_render_api::ModelHandle::NONE
        );
        assert_eq!(
            models.fx_handle("models/unknown.md2"),
            q2_render_api::ModelHandle::NONE
        );
    }

    #[test]
    fn prep_refresh_re_registers_models_whose_names_did_not_change() {
        // The renderer frees models not registered since begin_registration,
        // so a name the old map also used must be registered again.
        let mut renderer = Recorder::default();
        let mut models = ModelTable::default();
        prep_refresh(&mut renderer, demo2_strings, &mut models).unwrap();
        renderer.calls.clear();
        prep_refresh(&mut renderer, demo2_strings, &mut models).unwrap();
        assert!(renderer
            .calls
            .contains(&"model models/items/armor/jacket/tris.md2".to_string()));
    }

    #[test]
    fn prep_refresh_without_a_world_model_does_nothing() {
        let mut renderer = Recorder::default();
        prep_refresh(&mut renderer, |_| "", &mut ModelTable::default()).unwrap();
        assert!(renderer.calls.is_empty());
    }

    #[test]
    fn prep_refresh_stops_when_the_renderer_cannot_load_the_map() {
        let mut renderer = Recorder {
            fail_map: true,
            ..Recorder::default()
        };
        assert!(prep_refresh(&mut renderer, demo2_strings, &mut ModelTable::default()).is_err());
        assert_eq!(renderer.calls, ["begin demo2"]);
    }

    #[test]
    fn a_picture_frame_sets_up_2d_for_the_current_canvas_before_drawing() {
        let mut renderer = Recorder::default();
        picture_frame(&mut renderer, 640, 480, "victory.pcx");
        picture_frame(&mut renderer, 1280, 960, "victory.pcx");
        assert_eq!(
            renderer.calls,
            [
                "begin_frame 640 480",
                "stretch 0 0 640 480 pics/victory.pcx",
                "end_frame",
                "begin_frame 1280 960",
                "stretch 0 0 1280 960 pics/victory.pcx",
                "end_frame",
            ]
        );
    }

    #[test]
    fn a_picture_on_a_4_3_canvas_is_stretched_over_the_whole_view() {
        let mut renderer = Recorder::default();
        draw_picture(&mut renderer, 640, 480, "victory.pcx");
        assert_eq!(renderer.calls, ["stretch 0 0 640 480 pics/victory.pcx"]);
    }

    #[test]
    fn a_picture_on_a_wide_canvas_is_pillarboxed_to_4_3_like_c_cin_force43() {
        let mut renderer = Recorder::default();
        draw_picture(&mut renderer, 1280, 720, "victory.pcx");
        assert_eq!(
            renderer.calls,
            [
                "fill 0 0 160 720 0",
                "fill 1120 0 160 720 0",
                "stretch 160 0 960 720 pics/victory.pcx",
            ]
        );
    }

    #[test]
    fn a_picture_on_a_tall_canvas_is_letterboxed_with_a_width_rounded_down_to_4() {
        let mut renderer = Recorder::default();
        // w = 642 & !3 = 640, h = 480, x = 1, y = (1000 - 480) / 2.
        draw_picture(&mut renderer, 642, 1000, "victory.pcx");
        assert_eq!(
            renderer.calls,
            [
                "fill 0 0 1 1000 0",
                "fill 641 0 1 1000 0",
                "fill 1 0 640 260 0",
                "fill 1 740 640 260 0",
                "stretch 1 260 640 480 pics/victory.pcx",
            ]
        );
    }

    #[test]
    fn sky_axis_reads_three_floats_and_missing_ones_as_zero() {
        use q2_shared::types::Vec3f;
        assert_eq!(sky_axis("0.5 1 -2"), Vec3f::new(0.5, 1.0, -2.0));
        assert_eq!(sky_axis("1"), Vec3f::new(1.0, 0.0, 0.0));
        assert_eq!(sky_axis(""), Vec3f::ZERO);
    }

    #[test]
    fn model_table_registers_changed_names_once_and_skips_the_world() {
        use q2_shared::constants::CS_MODELS;
        let mut table = ModelTable::default();
        let mut registered = Vec::new();
        let strings = |i: usize| match i - CS_MODELS {
            1 => "maps/demo1.bsp",
            2 => "*1",
            3 => "models/monsters/soldier/tris.md2",
            _ => "",
        };
        let mut register = |name: &str| {
            registered.push(name.to_string());
            q2_render_api::ModelHandle::new(registered.len() as i32)
        };
        table.sync(strings, &mut register);
        table.sync(strings, &mut register);
        assert_eq!(registered, ["*1", "models/monsters/soldier/tris.md2"]);
        assert_eq!(table.handle(3), q2_render_api::ModelHandle::new(2));
        assert_eq!(table.handle(1), q2_render_api::ModelHandle::NONE);
        assert_eq!(table.handle(-4), q2_render_api::ModelHandle::NONE);
    }

    #[test]
    fn render_entities_leaves_out_unregistered_models() {
        use q2_client::entities::EntityView;
        let mut table = ModelTable::default();
        table.sync(
            |i| {
                if i == q2_shared::constants::CS_MODELS + 2 {
                    "*1"
                } else {
                    ""
                }
            },
            |_| q2_render_api::ModelHandle::new(7),
        );
        let view = |modelindex| EntityView {
            number: 0,
            effects: 0,
            modelindex,
            frame: 3,
            oldframe: 2,
            backlerp: 0.5,
            origin: q2_shared::types::Vec3f::ONE,
            oldorigin: q2_shared::types::Vec3f::ONE,
            angles: q2_shared::types::Vec3f::ZERO,
            skinnum: 1,
            flags: 0,
            alpha: 1.0,
        };
        let beam = EntityView {
            flags: q2_shared::constants::RF_BEAM,
            ..view(0)
        };
        let ents = render_entities(&[view(2), view(9), beam], &table);
        assert_eq!(ents.len(), 2, "a beam needs no model");
        assert_eq!(ents[1].model, q2_render_api::ModelHandle::NONE);
        assert_eq!(ents[0].model, q2_render_api::ModelHandle::new(7));
        assert_eq!(
            (ents[0].frame, ents[0].oldframe, ents[0].skinnum),
            (3, 2, 1)
        );
    }

    #[test]
    fn cmd_lightlevel_is_the_renderers_light_level_cast_to_a_byte_like_c() {
        assert_eq!(cmd_lightlevel(150.0), 150);
        assert_eq!(cmd_lightlevel(5.9), 5, "truncated toward zero");
        assert_eq!(cmd_lightlevel(0.0), 0);
        // C `(byte)cl_lightlevel->value` keeps the low 8 bits.
        assert_eq!(cmd_lightlevel(300.0), 44);
    }

    #[test]
    fn renderer_without_light_data_reports_the_player_lit() {
        use q2_render_api::Renderer;
        let renderer = Recorder::default();
        assert_eq!(cmd_lightlevel(renderer.light_level()), 150);
    }

    #[test]
    fn cmd_msec_is_the_frame_time_with_c_unreasonable_time_rule() {
        assert_eq!(cmd_msec(0.016), 16);
        assert_eq!(cmd_msec(0.25), 250);
        // CL_FinishMove: over 250 ms "time was unreasonable", send 100.
        assert_eq!(cmd_msec(0.4), 100);
        assert_eq!(cmd_msec(-1.0), 0);
    }

    #[test]
    fn query_values_are_found_by_their_exact_key() {
        assert_eq!(query_value("?map=demo3&hand=1", "map"), Some("demo3"));
        assert_eq!(query_value("?map=demo3&hand=1", "hand"), Some("1"));
        assert_eq!(query_value("?maps=x", "map"), None);
        assert_eq!(query_value("", "map"), None);
    }

    #[test]
    fn the_first_map_is_the_requested_one_else_demo1_else_the_first() {
        let maps = ["maps/demo1.bsp", "maps/demo3.bsp"];
        assert_eq!(start_map(Some("demo3"), &maps), Some("maps/demo3.bsp"));
        assert_eq!(
            start_map(Some("maps/demo3.bsp"), &maps),
            Some("maps/demo3.bsp")
        );
        assert_eq!(start_map(Some("base1"), &maps), Some("maps/demo1.bsp"));
        assert_eq!(start_map(None, &maps), Some("maps/demo1.bsp"));
        assert_eq!(start_map(None, &["maps/q2dm1.bsp"]), Some("maps/q2dm1.bsp"));
        assert_eq!(start_map(Some("demo3"), &[]), None);
    }

    #[test]
    fn hand_and_game_come_from_the_query_with_c_defaults() {
        assert_eq!(hand_from_query("?hand=1"), 1);
        assert_eq!(hand_from_query("?hand=2"), 2);
        assert_eq!(hand_from_query("?hand=left"), 0);
        assert_eq!(hand_from_query(""), 0);
        assert_eq!(local_userinfo(1), "\\hand\\1");
        assert_eq!(game_from_query("?game=rogue"), "rogue");
        // C cl_gun: 0 no gun, 1 none in a wide view, 2 (default) always.
        assert_eq!(cl_gun_from_query("?cl_gun=0"), "0");
        assert_eq!(cl_gun_from_query(""), "2");
        // Effects beyond C are opt-in.
        assert!(fx_extras_from_query("?cl_fxextra=1"));
        assert!(!fx_extras_from_query("?cl_fxextra=0"));
        assert!(!fx_extras_from_query(""));
        // C vid_fullscreen: 0 (default) windowed.
        assert!(vid_fullscreen_from_query("?vid_fullscreen=1"));
        assert!(!vid_fullscreen_from_query(""));
        assert_eq!(game_from_query(""), "");
    }

    #[test]
    fn vid_gamma_from_query_reads_the_gamma_parameter() {
        assert_eq!(vid_gamma_from_query("?gamma=1.2"), Some(1.2));
        assert_eq!(vid_gamma_from_query("?map=demo2&gamma=0.8"), Some(0.8));
        assert_eq!(vid_gamma_from_query("gamma=1.5"), Some(1.5));
    }

    #[test]
    fn vid_gamma_from_query_ignores_a_missing_or_unusable_value() {
        assert_eq!(vid_gamma_from_query(""), None);
        assert_eq!(vid_gamma_from_query("?map=demo2"), None);
        assert_eq!(vid_gamma_from_query("?gamma="), None);
        assert_eq!(vid_gamma_from_query("?gamma=bright"), None);
        assert_eq!(vid_gamma_from_query("?gamma=NaN"), None);
        assert_eq!(vid_gamma_from_query("?gamma=inf"), None);
        assert_eq!(vid_gamma_from_query("?gammas=1.5"), None);
    }

    #[test]
    fn vid_gamma_from_query_clamps_to_the_c_brightness_slider_range() {
        assert_eq!(vid_gamma_from_query("?gamma=9"), Some(VID_GAMMA_MAX));
        assert_eq!(vid_gamma_from_query("?gamma=0"), Some(VID_GAMMA_MIN));
        assert_eq!(vid_gamma_from_query("?gamma=-1"), Some(VID_GAMMA_MIN));
    }

    #[test]
    fn step_vid_gamma_moves_a_tenth_and_stays_in_range() {
        assert_eq!(step_vid_gamma(1.0, 1), 1.1);
        assert_eq!(step_vid_gamma(1.0, -1), 0.9);
        assert_eq!(step_vid_gamma(1.0, 0), 1.0);
        assert_eq!(step_vid_gamma(VID_GAMMA_MAX, 1), VID_GAMMA_MAX);
        assert_eq!(step_vid_gamma(VID_GAMMA_MIN, -1), VID_GAMMA_MIN);
        // Ten steps up from 1.0 land on 2.0 exactly, not 1.9999.
        let stepped = (0..10).fold(1.0, |v, _| step_vid_gamma(v, 1));
        assert_eq!(stepped, 2.0);
    }

    #[test]
    fn gamma_key_step_counts_f8_brighter_and_f7_darker_once_per_press() {
        use q2_platform::keymap::{K_F7, K_F8};
        let up = [false; 256];
        let mut f8 = up;
        f8[K_F8 as usize] = true;
        let mut f7 = up;
        f7[K_F7 as usize] = true;
        assert_eq!(gamma_key_step(&up, &f8), 1);
        assert_eq!(gamma_key_step(&up, &f7), -1);
        // Held from the last frame: no repeat.
        assert_eq!(gamma_key_step(&f8, &f8), 0);
        assert_eq!(gamma_key_step(&f8, &up), 0);
        let mut both = f8;
        both[K_F7 as usize] = true;
        assert_eq!(gamma_key_step(&up, &both), 0);
    }

    fn pressed(keys: &[u8]) -> [bool; 256] {
        let mut down = [false; 256];
        for &k in keys {
            down[k as usize] = true;
        }
        down
    }

    #[test]
    fn the_view_weapon_is_drawn_as_a_depth_hacked_weapon_model() {
        use q2_shared::constants::{CS_MODELS, RF_DEPTHHACK, RF_MINLIGHT, RF_WEAPONMODEL};
        use q2_shared::types::Vec3f;
        let mut table = ModelTable::default();
        table.sync(
            |i| {
                if i == CS_MODELS + 4 {
                    "models/weapons/v_blast/tris.md2"
                } else {
                    ""
                }
            },
            |_| q2_render_api::ModelHandle::new(9),
        );
        let gun = q2_client::frame::ViewWeapon {
            modelindex: 4,
            origin: Vec3f::new(1.0, 2.0, 3.0),
            angles: Vec3f::new(0.0, 90.0, 0.0),
            frame: 7,
            oldframe: 6,
            backlerp: 0.25,
        };
        let ent = gun_entity(&gun, &table).unwrap();
        assert_eq!(ent.model, q2_render_api::ModelHandle::new(9));
        assert_eq!(ent.flags, RF_MINLIGHT | RF_DEPTHHACK | RF_WEAPONMODEL);
        assert_eq!((ent.frame, ent.oldframe, ent.backlerp), (7, 6, 0.25));
        assert_eq!(ent.origin, gun.origin);
        assert_eq!(ent.oldorigin, gun.origin, "don't lerp at all");
        assert_eq!(ent.angles, gun.angles);

        let unknown = q2_client::frame::ViewWeapon {
            modelindex: 5,
            ..gun
        };
        assert!(gun_entity(&unknown, &table).is_none());
    }

    #[test]
    fn effect_entities_need_their_model_except_beams() {
        use q2_client::fx::{FxEntity, MOD_FLASH, MOD_SMOKE};
        use q2_shared::types::Vec3f;
        let mut table = ModelTable::default();
        table.register_fx(|name| {
            if name == MOD_SMOKE {
                q2_render_api::ModelHandle::new(5)
            } else {
                q2_render_api::ModelHandle::NONE
            }
        });
        let laser = FxEntity {
            origin: Vec3f::ONE,
            oldorigin: Vec3f::ZERO,
            frame: 4,
            skinnum: 0xd0,
            alpha: 0.3,
            flags: q2_shared::constants::RF_BEAM | q2_shared::constants::RF_TRANSLUCENT,
            ..FxEntity::default()
        };
        let smoke = FxEntity {
            model: Some(MOD_SMOKE),
            frame: 2,
            alpha: 0.5,
            ..FxEntity::default()
        };
        let unloaded = FxEntity {
            model: Some(MOD_FLASH),
            ..FxEntity::default()
        };
        let ents = fx_render_entities(&[laser, smoke, unloaded], &table);
        assert_eq!(ents.len(), 2);
        assert_eq!(ents[0].model, q2_render_api::ModelHandle::NONE);
        assert_eq!(
            (
                ents[0].oldorigin,
                ents[0].frame,
                ents[0].skinnum,
                ents[0].alpha
            ),
            (Vec3f::ZERO, 4, 0xd0, 0.3)
        );
        assert_eq!(ents[1].model, q2_render_api::ModelHandle::new(5));
        assert_eq!((ents[1].frame, ents[1].alpha), (2, 0.5));
    }

    #[test]
    fn power_screens_cover_other_entities_but_not_the_viewer() {
        use q2_client::entities::EntityView;
        use q2_shared::constants::{EF_POWERSCREEN, RF_VIEWERMODEL};
        let view = |effects, flags| EntityView {
            number: 3,
            effects,
            modelindex: 2,
            frame: 5,
            oldframe: 4,
            backlerp: 0.5,
            origin: q2_shared::types::Vec3f::ONE,
            oldorigin: q2_shared::types::Vec3f::ONE,
            angles: q2_shared::types::Vec3f::ZERO,
            skinnum: 0,
            flags,
            alpha: 1.0,
        };
        let screens = power_screens(&[
            view(EF_POWERSCREEN, 0),
            view(0, 0),
            view(EF_POWERSCREEN, RF_VIEWERMODEL),
        ]);
        assert_eq!(screens.len(), 1);
        assert_eq!(screens[0].model, Some(q2_client::fx::MOD_POWERSCREEN));
        assert_eq!(
            (screens[0].frame, screens[0].origin),
            (0, q2_shared::types::Vec3f::ONE)
        );
    }

    #[test]
    fn effect_particles_lights_and_sounds_reach_the_renderer_and_mixer() {
        use q2_client::fx::{FxLight, FxSound};
        use q2_client::particles::DrawParticle;
        use q2_shared::types::Vec3f;
        let particles = fx_particles(&[DrawParticle {
            origin: Vec3f::ONE,
            color: 0xe0,
            alpha: 0.25,
        }]);
        assert_eq!(particles.len(), 1);
        let p = &particles[0];
        assert_eq!((p.origin, p.color, p.alpha), (Vec3f::ONE, 0xe0, 0.25));
        let lights = fx_dlights(&[FxLight {
            origin: Vec3f::ONE,
            intensity: 200.0,
            color: [1.0, 1.0, 0.0],
        }]);
        assert_eq!(lights[0].intensity, 200.0);
        assert_eq!(lights[0].color, Vec3f::new(1.0, 1.0, 0.0));
        let sound = FxSound {
            name: "weapons/rocklx1a.wav",
            origin: Some(Vec3f::ONE),
            entnum: 0,
            channel: 0,
            volume: 1.0,
            attenuation: 1.0,
            timeofs: 0.0,
        };
        let named = fx_named_sound(&sound);
        assert_eq!((named.name, named.origin), (sound.name, sound.origin));
        let talk = talk_sound();
        let named = flash_named_sound(&talk);
        assert_eq!(
            (named.name, named.origin, named.entnum),
            ("misc/talk.wav", None, talk.entnum)
        );
    }

    #[test]
    fn lit_flash_lights_reach_the_renderer_with_radius_as_intensity() {
        use q2_shared::types::{EntityState, Vec3f};
        let mut effects = q2_client::effects::Effects::default();
        assert!(render_dlights(&effects).is_empty());
        let player = EntityState {
            number: 1,
            ..Default::default()
        };
        effects.muzzle_flash(&player, q2_shared::constants::MZ_RAILGUN, 0.0, &mut || 3);
        let lights = render_dlights(&effects);
        assert_eq!(lights.len(), 1);
        assert_eq!(lights[0].intensity, 203.0);
        assert_eq!(lights[0].color, Vec3f::new(0.5, 0.5, 1.0));
        assert_eq!(lights[0].origin, Vec3f::new(18.0, -16.0, 0.0));
    }

    #[test]
    fn xorshift_never_sticks_and_varies() {
        let mut state = 1;
        let draws: Vec<u32> = (0..4).map(|_| xorshift(&mut state)).collect();
        assert!(draws.iter().all(|&d| d != 0));
        assert_ne!(draws[0], draws[1]);
    }

    #[test]
    fn randk_is_never_negative_like_c() {
        let mut state = 0x8000_0000;
        assert!((0..1000).all(|_| xorshift(&mut state) <= i32::MAX as u32));
    }

    #[test]
    fn number_keys_use_weapons_like_q2_default_cfg() {
        let none = [false; 256];
        assert_eq!(bound_commands(&none, &pressed(b"1")), ["use Blaster"]);
        assert_eq!(bound_commands(&none, &pressed(b"3")), ["use Super Shotgun"]);
        assert_eq!(bound_commands(&none, &pressed(b"0")), ["use BFG10K"]);
    }

    #[test]
    fn f1_opens_the_help_computer_like_q2_default_cfg() {
        use q2_platform::keymap::K_F1;
        assert_eq!(bound_commands(&[false; 256], &pressed(&[K_F1])), ["help"]);
    }

    #[test]
    fn chat_is_greeted_with_the_talk_sound_heard_everywhere() {
        let talk = talk_sound();
        assert_eq!(
            (talk.name.as_str(), talk.attenuation),
            ("misc/talk.wav", 0.0)
        );
    }

    #[test]
    fn inventory_keys_bind_like_q2_default_cfg() {
        use q2_platform::keymap::K_ENTER;
        let none = [false; 256];
        assert_eq!(bound_commands(&none, &pressed(b"[")), ["invprev"]);
        assert_eq!(bound_commands(&none, &pressed(b"]")), ["invnext"]);
        assert_eq!(bound_commands(&none, &pressed(b"'")), ["invdrop"]);
        assert_eq!(bound_commands(&none, &pressed(&[K_ENTER])), ["invuse"]);
    }

    #[test]
    fn a_bound_key_runs_its_command_once_per_press() {
        let held = pressed(b"2");
        assert!(bound_commands(&held, &held).is_empty());
        assert!(bound_commands(&held, &[false; 256]).is_empty());
        assert!(bound_commands(&[false; 256], &pressed(b"w")).is_empty());
        assert_eq!(
            bound_commands(&[false; 256], &pressed(b"12")),
            ["use Blaster", "use Shotgun"]
        );
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
    fn frame_dt_is_capped_at_half_a_second_like_c_nframetime() {
        assert_eq!(frame_dt_seconds(5000.0, 1000.0), 0.5);
        assert!((frame_dt_seconds(1300.0, 1000.0) - 0.3).abs() < 1e-6);
    }

    #[test]
    fn a_stall_over_250ms_sends_the_c_unreasonable_time_of_100() {
        assert_eq!(cmd_msec(frame_dt_seconds(1300.0, 1000.0)), 100);
    }

    #[test]
    fn elapsed_microseconds_is_uncapped_and_never_negative() {
        assert_eq!(elapsed_microseconds(1006.944, 1000.0), 6944);
        assert_eq!(elapsed_microseconds(3000.0, 1000.0), 2_000_000);
        assert_eq!(elapsed_microseconds(1000.0, 1010.0), 0);
    }

    #[test]
    fn turn_view_speed_key_turns_at_cl_anglespeedkey_times_the_speed() {
        use q2_platform::keymap::{K_LEFTARROW, K_SHIFT};
        let mut input = q2_client::input::InputState::default();
        let mut keys = [false; 256];
        keys[K_LEFTARROW as usize] = true;
        keys[K_SHIFT as usize] = true;
        turn_view(&mut input, &keys, 0.0, 0.0, 0.5, 0.0);
        assert!(
            (input.view_angles.y - 105.0).abs() < 1e-4,
            "140 deg/s * 1.5 for 0.5 s, got {}",
            input.view_angles.y
        );
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
