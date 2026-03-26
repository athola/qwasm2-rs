use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

/// Callback type for the game frame.
pub type FrameCallback = Box<dyn FnMut(f64)>;

/// Start the requestAnimationFrame game loop.
pub fn start_game_loop(mut callback: FrameCallback) -> Result<(), String> {
    let f: Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>> = Rc::new(RefCell::new(None));
    let g = f.clone();

    *g.borrow_mut() = Some(Closure::wrap(Box::new(move |timestamp: f64| {
        // Call the game frame
        callback(timestamp);

        // Schedule next frame
        request_animation_frame(f.borrow().as_ref().unwrap());
    }) as Box<dyn FnMut(f64)>));

    request_animation_frame(g.borrow().as_ref().unwrap());
    Ok(())
}

fn request_animation_frame(f: &Closure<dyn FnMut(f64)>) {
    web_sys::window()
        .expect("no window")
        .request_animation_frame(f.as_ref().unchecked_ref())
        .expect("request_animation_frame failed");
}

/// Get high-resolution time in milliseconds.
pub fn performance_now() -> f64 {
    web_sys::window()
        .expect("no window")
        .performance()
        .expect("no performance")
        .now()
}
