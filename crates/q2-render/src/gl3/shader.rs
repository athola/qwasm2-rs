//! The single world/sky shader program (C `gl3_shaders.c`: `fragmentSrc3Dlm`,
//! `fragmentSrc3Dwater`, `fragmentSrc3D`, `fragmentSrc3Dsky`, merged behind a
//! `u_mode` switch so one program serves every world draw).
//!
//! Colour constants are the C cvar defaults: `gl3_intensity` 1.5,
//! `gl3_overbrightbits` 1.3 and `vid_gamma` 1.0 (applied as `pow(c, 1/gamma)`;
//! GL3 asks for 1.2, but `VID_Init` registers the cvar with 1.0 first).
//! Lightmap layer `n` is scaled by the brightness of the face's `n`th light
//! style (C `lmScales`), then the sum by `overbrightbits`.

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
layout(location = 3) in vec4 a_styles;
uniform mat4 u_view_proj;
uniform int u_mode;
uniform float u_scroll;
// Light style brightness, four styles per vector (C lightstyles[].rgb).
uniform vec4 u_styles[64];
out vec2 v_uv;
out vec2 v_lightmap_uv;
out vec4 v_style_scales;
float style_scale(float style) {
    // -1 marks an unused layer; its texels are zero, so any scale works.
    if (style < 0.0) {
        return 1.0;
    }
    int index = int(style);
    return u_styles[index / 4][index % 4];
}
void main() {
    v_style_scales = vec4(style_scale(a_styles.x), style_scale(a_styles.y),
                          style_scale(a_styles.z), style_scale(a_styles.w));
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
uniform sampler2D u_lightmap0;
uniform sampler2D u_lightmap1;
uniform sampler2D u_lightmap2;
uniform sampler2D u_lightmap3;
uniform int u_mode;
uniform float u_alpha;
uniform float u_time;
uniform float u_scroll;
uniform float u_turb_light;
in vec2 v_uv;
in vec2 v_lightmap_uv;
in vec4 v_style_scales;
out vec4 o_color;

const float GAMMA = 1.0 / 1.0;
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
        vec3 light = texture(u_lightmap0, v_lightmap_uv).rgb * v_style_scales.x
                   + texture(u_lightmap1, v_lightmap_uv).rgb * v_style_scales.y
                   + texture(u_lightmap2, v_lightmap_uv).rgb * v_style_scales.z
                   + texture(u_lightmap3, v_lightmap_uv).rgb * v_style_scales.w;
        light *= OVERBRIGHT;
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
    pub styles: Option<glow::UniformLocation>,
}

/// Compile and link one program from vertex and fragment source. Shader
/// objects never outlive the call; the log is reported on failure.
fn link_program(
    gl: &glow::Context,
    vertex_src: &str,
    fragment_src: &str,
) -> Result<glow::Program, String> {
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
        gl.shader_source(vs, vertex_src);
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
        gl.shader_source(fs, fragment_src);
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
        Ok(program)
    }
}

/// Bind the named samplers of `program` to texture units 0, 1, ...
fn bind_samplers(gl: &glow::Context, program: glow::Program, names: &[&str]) {
    // SAFETY: `program` was linked on `gl` by the caller; this only sets
    // sampler uniforms on it and restores the null program binding.
    unsafe {
        gl.use_program(Some(program));
        for (unit, name) in (0..).zip(names) {
            gl.uniform_1_i32(gl.get_uniform_location(program, name).as_ref(), unit);
        }
        gl.use_program(None);
    }
}

/// Compile and link the world program; texture units are bound to
/// `u_texture` = 0 and `u_lightmap0..3` = 1..4.
pub fn compile_world_program(gl: &glow::Context) -> Result<(glow::Program, WorldUniforms), String> {
    let program = link_program(gl, VERTEX_SRC, FRAGMENT_SRC)?;
    bind_samplers(
        gl,
        program,
        &[
            "u_texture",
            "u_lightmap0",
            "u_lightmap1",
            "u_lightmap2",
            "u_lightmap3",
        ],
    );
    // SAFETY: `program` is the linked program above; uniform lookups only
    // read from it.
    let uniforms = unsafe {
        WorldUniforms {
            view_proj: gl.get_uniform_location(program, "u_view_proj"),
            mode: gl.get_uniform_location(program, "u_mode"),
            alpha: gl.get_uniform_location(program, "u_alpha"),
            time: gl.get_uniform_location(program, "u_time"),
            scroll: gl.get_uniform_location(program, "u_scroll"),
            turb_light: gl.get_uniform_location(program, "u_turb_light"),
            styles: gl.get_uniform_location(program, "u_styles"),
        }
    };
    Ok((program, uniforms))
}

// C `vertexSrcAlias` / `fragmentSrcAlias`: vertex colour carries the shade
// and alpha, scaled by overbrightbits; the fragment clamps it to 1.5.
const ALIAS_VERTEX_SRC: &str = r#"#version 300 es
precision highp float;
layout(location = 0) in vec3 a_position;
layout(location = 1) in vec2 a_uv;
layout(location = 2) in vec4 a_color;
uniform mat4 u_mvp;
out vec2 v_uv;
out vec4 v_color;
const float OVERBRIGHT = 1.3;
void main() {
    v_color = a_color * OVERBRIGHT;
    v_uv = a_uv;
    gl_Position = u_mvp * vec4(a_position, 1.0);
}
"#;

const ALIAS_FRAGMENT_SRC: &str = r#"#version 300 es
precision highp float;
uniform sampler2D u_texture;
in vec2 v_uv;
in vec4 v_color;
out vec4 o_color;
const float GAMMA = 1.0 / 1.0;
const float INTENSITY = 1.5;
void main() {
    vec4 texel = texture(u_texture, v_uv);
    texel.rgb *= INTENSITY;
    texel *= min(vec4(1.5), v_color);
    o_color = vec4(pow(texel.rgb, vec3(GAMMA)), texel.a);
}
"#;

/// Uniform locations of the alias (MD2) program.
pub struct AliasUniforms {
    pub mvp: Option<glow::UniformLocation>,
}

/// Link the alias program; `u_texture` is unit 0.
pub fn compile_alias_program(gl: &glow::Context) -> Result<(glow::Program, AliasUniforms), String> {
    let program = link_program(gl, ALIAS_VERTEX_SRC, ALIAS_FRAGMENT_SRC)?;
    bind_samplers(gl, program, &["u_texture"]);
    // SAFETY: `program` is the linked program above; the lookup only reads it.
    let mvp = unsafe { gl.get_uniform_location(program, "u_mvp") };
    Ok((program, AliasUniforms { mvp }))
}

// C `vertexSrc2D` / `fragmentSrc2D` / `fragmentSrc2Dcolor` in one program:
// fills sample a white texel, so colour is always texel * vertex colour.
const UI_VERTEX_SRC: &str = r#"#version 300 es
precision highp float;
layout(location = 0) in vec2 a_position;
layout(location = 1) in vec2 a_uv;
layout(location = 2) in vec4 a_color;
uniform mat4 u_ortho;
out vec2 v_uv;
out vec4 v_color;
void main() {
    v_uv = a_uv;
    v_color = a_color;
    gl_Position = u_ortho * vec4(a_position, 0.0, 1.0);
}
"#;

const UI_FRAGMENT_SRC: &str = r#"#version 300 es
precision highp float;
uniform sampler2D u_texture;
in vec2 v_uv;
in vec4 v_color;
out vec4 o_color;
const float GAMMA = 1.0 / 1.0;
const float INTENSITY_2D = 1.5;
void main() {
    vec4 texel = texture(u_texture, v_uv);
    // C: glAlphaFunc(GL_GREATER, 0.666) equivalent.
    if (texel.a <= 0.666) {
        discard;
    }
    vec3 rgb = texel.rgb * v_color.rgb * INTENSITY_2D;
    o_color = vec4(pow(rgb, vec3(GAMMA)), texel.a * v_color.a);
}
"#;

/// Uniform locations of the 2D program.
pub struct UiUniforms {
    pub ortho: Option<glow::UniformLocation>,
}

/// Link the 2D program; `u_texture` is unit 0.
pub fn compile_ui_program(gl: &glow::Context) -> Result<(glow::Program, UiUniforms), String> {
    let program = link_program(gl, UI_VERTEX_SRC, UI_FRAGMENT_SRC)?;
    bind_samplers(gl, program, &["u_texture"]);
    // SAFETY: `program` is the linked program above; the lookup only reads it.
    let ortho = unsafe { gl.get_uniform_location(program, "u_ortho") };
    Ok((program, UiUniforms { ortho }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_fragment_shader_uses_the_effective_c_gamma_of_one() {
        // VID_Init registers vid_gamma "1.0" before GL3 asks for "1.2", and
        // Cvar_Get keeps the first default, so C's gamma is 1/1.0.
        for source in [FRAGMENT_SRC, ALIAS_FRAGMENT_SRC, UI_FRAGMENT_SRC] {
            assert!(source.contains("const float GAMMA = 1.0 / 1.0;"));
        }
    }
}
