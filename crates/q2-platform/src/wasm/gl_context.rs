use super::fatal;
use q2_common::error::Q2Result;
use wasm_bindgen::JsCast;
use web_sys::{HtmlCanvasElement, WebGl2RenderingContext};

/// Create a glow::Context from a canvas element ID.
pub fn create_webgl2_context(canvas_id: &str) -> Q2Result<(glow::Context, HtmlCanvasElement)> {
    let window = web_sys::window().ok_or_else(|| fatal("no window"))?;
    let document = window.document().ok_or_else(|| fatal("no document"))?;

    let canvas = document
        .get_element_by_id(canvas_id)
        .ok_or_else(|| fatal(format!("no element with id '{}'", canvas_id)))?;
    let canvas: HtmlCanvasElement = canvas
        .dyn_into()
        .map_err(|_| fatal("element is not a canvas"))?;

    // No alpha channel: the renderer blends colour only, as C does on a
    // desktop framebuffer whose alpha is ignored. With the default
    // `alpha: true`, translucent draws would leave framebuffer alpha below 1
    // and the page would show through when the browser composites the
    // canvas.
    let attributes = js_sys::Object::new();
    js_sys::Reflect::set(&attributes, &"alpha".into(), &false.into())
        .map_err(|e| fatal(format!("context attributes: {:?}", e)))?;
    let webgl2: WebGl2RenderingContext = canvas
        .get_context_with_context_options("webgl2", &attributes)
        .map_err(|e| fatal(format!("get_context error: {:?}", e)))?
        .ok_or_else(|| fatal("WebGL2 not supported"))?
        .dyn_into()
        .map_err(|_| fatal("not a WebGL2 context"))?;

    // Create glow context from WebGL2
    let gl = glow::Context::from_webgl2_context(webgl2);

    Ok((gl, canvas))
}
