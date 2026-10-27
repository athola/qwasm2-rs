//! The single world/sky shader program (C `gl3_shaders.c`: `fragmentSrc3Dlm`,
//! `fragmentSrc3Dwater`, `fragmentSrc3D`, `fragmentSrc3Dsky`, merged behind a
//! `u_mode` switch so one program serves every world draw).
//!
//! Colour constants are the C cvar defaults: `gl3_intensity` 1.5,
//! `gl3_overbrightbits` 1.3 and `vid_gamma` 1.2 (applied as `pow(c, 1/gamma)`).
//! In C the lightmap scale is `lmScales[0] = 1` for the all-white style table,
//! so a style-0 lightmap sample is multiplied by `overbrightbits` only.

use glow::HasContext;

pub const MODE_LIT: i32 = 0;
pub const MODE_PLAIN: i32 = 1;
pub const MODE_WARP: i32 = 2;
pub const MODE_SKY: i32 = 3;

const VERTEX_SRC: &str = r#"#version 300 es
precision highp float;
layout(location = 0) in vec3 a_position;
layout(location = 1) in vec2 a_uv;
layout(location = 2) in vec2 a_lightmap_uv;
uniform mat4 u_view_proj;
uniform int u_mode;
uniform float u_scroll;
out vec2 v_uv;
out vec2 v_lightmap_uv;
void main() {
    v_uv = a_uv;
    // SURF_FLOWING shifts s in the vertex stage; warp applies it after the wobble.
    if (u_mode != 2) {
        v_uv.x += u_scroll;
    }
    v_lightmap_uv = a_lightmap_uv;
    gl_Position = u_view_proj * vec4(a_position, 1.0);
}
"#;

const FRAGMENT_SRC: &str = r#"#version 300 es
precision highp float;
// Uniforms shared with the vertex stage must have the same precision there.
precision highp int;
uniform sampler2D u_texture;
uniform sampler2D u_lightmap;
uniform int u_mode;
uniform float u_alpha;
uniform float u_time;
uniform float u_scroll;
uniform float u_turb_light;
in vec2 v_uv;
in vec2 v_lightmap_uv;
out vec4 o_color;

const float GAMMA = 1.0 / 1.2;
const float INTENSITY = 1.5;
const float OVERBRIGHT = 1.3;

void main() {
    if (u_mode == 2) {
        // gl3_shaders.c fragmentSrc3Dwater: sine wobble on raw texel coordinates.
        vec2 tc = v_uv;
        tc.x += sin(v_uv.y * 0.125 + u_time) * 4.0;
        tc.x += u_scroll;
        tc.y += sin(v_uv.x * 0.125 + u_time) * 4.0;
        tc *= 1.0 / 64.0;
        vec4 texel = texture(u_texture, tc);
        texel.rgb *= INTENSITY * u_turb_light;
        o_color = vec4(pow(texel.rgb, vec3(GAMMA)), texel.a * u_alpha);
    } else if (u_mode == 0) {
        vec4 texel = texture(u_texture, v_uv);
        texel.rgb *= INTENSITY;
        vec3 light = texture(u_lightmap, v_lightmap_uv).rgb * OVERBRIGHT;
        o_color = vec4(pow(light * texel.rgb, vec3(GAMMA)), 1.0);
    } else if (u_mode == 3) {
        vec4 texel = texture(u_texture, v_uv);
        o_color = vec4(pow(texel.rgb, vec3(GAMMA)), 1.0);
    } else {
        vec4 texel = texture(u_texture, v_uv);
        texel.rgb *= INTENSITY;
        o_color = vec4(pow(texel.rgb, vec3(GAMMA)), texel.a * u_alpha);
    }
}
"#;

/// Uniform locations of the world program.
pub struct WorldUniforms {
    pub view_proj: Option<glow::UniformLocation>,
    pub mode: Option<glow::UniformLocation>,
    pub alpha: Option<glow::UniformLocation>,
    pub time: Option<glow::UniformLocation>,
    pub scroll: Option<glow::UniformLocation>,
    pub turb_light: Option<glow::UniformLocation>,
}

/// Compile and link the world program; texture units are bound to
/// `u_texture` = 0 and `u_lightmap` = 1.
pub fn compile_world_program(gl: &glow::Context) -> Result<(glow::Program, WorldUniforms), String> {
    // SAFETY: Every shader and program handle is created on `gl` within
    // this block and deleted on it on each error path; only the linked
    // program escapes, to a caller that owns the same context.
    unsafe {
        let program = gl
            .create_program()
            .map_err(|e| format!("create_program failed: {e}"))?;

        let vs = gl.create_shader(glow::VERTEX_SHADER).map_err(|e| {
            gl.delete_program(program);
            format!("create vertex shader failed: {e}")
        })?;
        gl.shader_source(vs, VERTEX_SRC);
        gl.compile_shader(vs);
        if !gl.get_shader_compile_status(vs) {
            let log = gl.get_shader_info_log(vs);
            gl.delete_shader(vs);
            gl.delete_program(program);
            let msg = format!("VS compile error: {}", log);
            tracing::error!("{}", msg);
            return Err(msg);
        }

        let fs = gl.create_shader(glow::FRAGMENT_SHADER).map_err(|e| {
            gl.delete_shader(vs);
            gl.delete_program(program);
            format!("create fragment shader failed: {e}")
        })?;
        gl.shader_source(fs, FRAGMENT_SRC);
        gl.compile_shader(fs);
        if !gl.get_shader_compile_status(fs) {
            let log = gl.get_shader_info_log(fs);
            gl.delete_shader(fs);
            gl.delete_shader(vs);
            gl.delete_program(program);
            let msg = format!("FS compile error: {}", log);
            tracing::error!("{}", msg);
            return Err(msg);
        }

        gl.attach_shader(program, vs);
        gl.attach_shader(program, fs);
        gl.link_program(program);
        if !gl.get_program_link_status(program) {
            let log = gl.get_program_info_log(program);
            gl.delete_shader(vs);
            gl.delete_shader(fs);
            gl.delete_program(program);
            let msg = format!("Program link error: {}", log);
            tracing::error!("{}", msg);
            return Err(msg);
        }

        gl.delete_shader(vs);
        gl.delete_shader(fs);

        gl.use_program(Some(program));
        gl.uniform_1_i32(gl.get_uniform_location(program, "u_texture").as_ref(), 0);
        gl.uniform_1_i32(gl.get_uniform_location(program, "u_lightmap").as_ref(), 1);
        gl.use_program(None);

        let uniforms = WorldUniforms {
            view_proj: gl.get_uniform_location(program, "u_view_proj"),
            mode: gl.get_uniform_location(program, "u_mode"),
            alpha: gl.get_uniform_location(program, "u_alpha"),
            time: gl.get_uniform_location(program, "u_time"),
            scroll: gl.get_uniform_location(program, "u_scroll"),
            turb_light: gl.get_uniform_location(program, "u_turb_light"),
        };
        Ok((program, uniforms))
    }
}
