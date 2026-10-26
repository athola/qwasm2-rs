use super::fatal;
use q2_common::error::Q2Result;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

/// Callback type for the game frame.
pub type FrameCallback = Box<dyn FnMut(f64)>;

/// Self-referential slot: the RAF closure reschedules itself through it.
type RafSlot = Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>>;

/// Start the requestAnimationFrame game loop.
pub fn start_game_loop(mut callback: FrameCallback) -> Q2Result<()> {
    let f: RafSlot = Rc::new(RefCell::new(None));
    let g = f.clone();

    *g.borrow_mut() = Some(Closure::wrap(Box::new(move |timestamp: f64| {
        // Call the game frame
        callback(timestamp);

        // Schedule next frame
        if let Err(e) = request_animation_frame(f.borrow().as_ref().unwrap()) {
            web_sys::console::error_1(&format!("[qwasm2-rs] RAF error: {}", e).into());
        }
    }) as Box<dyn FnMut(f64)>));

    request_animation_frame(g.borrow().as_ref().unwrap())
        .map_err(|e| fatal(format!("initial RAF failed: {}", e)))?;
    Ok(())
}

fn request_animation_frame(f: &Closure<dyn FnMut(f64)>) -> Q2Result<i32> {
    web_sys::window()
        .ok_or_else(|| fatal("no window"))?
        .request_animation_frame(f.as_ref().unchecked_ref())
        .map_err(|e| fatal(format!("request_animation_frame failed: {:?}", e)))
}

/// Get high-resolution time in milliseconds.
///
/// Returns 0.0 if the window or performance API is unavailable.
pub fn performance_now() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or(0.0)
}
