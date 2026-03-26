use wasm_bindgen::JsCast;
use web_sys::{HtmlCanvasElement, WebGl2RenderingContext};

/// Create a glow::Context from a canvas element ID.
pub fn create_webgl2_context(
    canvas_id: &str,
) -> Result<(glow::Context, HtmlCanvasElement), String> {
    let window = web_sys::window().ok_or("no window")?;
    let document = window.document().ok_or("no document")?;

    let canvas = document
        .get_element_by_id(canvas_id)
        .ok_or_else(|| format!("no element with id '{}'", canvas_id))?;
    let canvas: HtmlCanvasElement = canvas.dyn_into().map_err(|_| "element is not a canvas")?;

    let webgl2: WebGl2RenderingContext = canvas
        .get_context("webgl2")
        .map_err(|e| format!("get_context error: {:?}", e))?
        .ok_or("WebGL2 not supported")?
        .dyn_into()
        .map_err(|_| "not a WebGL2 context")?;

    // Create glow context from WebGL2
    let gl = glow::Context::from_webgl2_context(webgl2);

    Ok((gl, canvas))
}
