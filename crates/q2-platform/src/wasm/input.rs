use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Event, HtmlCanvasElement, KeyboardEvent, MouseEvent};

pub use crate::keymap::key_code_to_q2;

/// Shared input state that event listeners write to.
#[derive(Debug)]
pub struct WasmInputState {
    pub keys: [bool; 256],
    pub mouse_dx: f32,
    pub mouse_dy: f32,
    pub mouse_buttons: u8,
}

impl Default for WasmInputState {
    fn default() -> Self {
        Self {
            keys: [false; 256],
            mouse_dx: 0.0,
            mouse_dy: 0.0,
            mouse_buttons: 0,
        }
    }
}

/// Set up keyboard and mouse event listeners on a canvas.
/// Returns a shared reference to the input state.
pub fn setup_input_listeners(
    canvas: &HtmlCanvasElement,
) -> Result<Rc<RefCell<WasmInputState>>, String> {
    let state = Rc::new(RefCell::new(WasmInputState::default()));
    let document = web_sys::window()
        .ok_or("no window")?
        .document()
        .ok_or("no document")?;

    // Keyboard down
    {
        let state = state.clone();
        let closure = Closure::wrap(Box::new(move |event: KeyboardEvent| {
            event.prevent_default();
            if let Some(key) = key_code_to_q2(&event.code()) {
                state.borrow_mut().keys[key as usize] = true;
            }
        }) as Box<dyn FnMut(_)>);
        document
            .add_event_listener_with_callback("keydown", closure.as_ref().unchecked_ref())
            .map_err(|e| format!("keydown listener: {:?}", e))?;
        closure.forget(); // leak -- lives for lifetime of page
    }

    // Keyboard up
    {
        let state = state.clone();
        let closure = Closure::wrap(Box::new(move |event: KeyboardEvent| {
            event.prevent_default();
            if let Some(key) = key_code_to_q2(&event.code()) {
                state.borrow_mut().keys[key as usize] = false;
            }
        }) as Box<dyn FnMut(_)>);
        document
            .add_event_listener_with_callback("keyup", closure.as_ref().unchecked_ref())
            .map_err(|e| format!("keyup listener: {:?}", e))?;
        closure.forget();
    }

    // Mouse move — listen on document so events are received regardless of cursor position.
    // Only accumulate deltas while pointer lock is active; without lock, movementX/Y can
    // produce a large spurious jump when the cursor re-enters the viewport boundary.
    {
        let state = state.clone();
        let document_for_move = document.clone();
        let closure = Closure::wrap(Box::new(move |event: MouseEvent| {
            if document_for_move.pointer_lock_element().is_some() {
                let mut s = state.borrow_mut();
                s.mouse_dx += event.movement_x() as f32;
                s.mouse_dy += event.movement_y() as f32;
            }
        }) as Box<dyn FnMut(_)>);
        document
            .add_event_listener_with_callback("mousemove", closure.as_ref().unchecked_ref())
            .map_err(|e| format!("mousemove listener: {:?}", e))?;
        closure.forget();
    }

    // Flush stale deltas whenever pointer lock changes state (acquire or release).
    // Prevents a view jump from deltas that accumulated in an unlocked window.
    {
        let state = state.clone();
        let closure = Closure::wrap(Box::new(move |_: Event| {
            let mut s = state.borrow_mut();
            s.mouse_dx = 0.0;
            s.mouse_dy = 0.0;
        }) as Box<dyn FnMut(_)>);
        document
            .add_event_listener_with_callback("pointerlockchange", closure.as_ref().unchecked_ref())
            .map_err(|e| format!("pointerlockchange listener: {:?}", e))?;
        closure.forget();
    }

    // Log pointer lock errors to the browser console for diagnostics.
    {
        let closure = Closure::wrap(Box::new(move |_: Event| {
            web_sys::console::warn_1(&"[qwasm2] Pointer lock request failed (browser policy or missing user gesture)".into());
        }) as Box<dyn FnMut(_)>);
        document
            .add_event_listener_with_callback("pointerlockerror", closure.as_ref().unchecked_ref())
            .map_err(|e| format!("pointerlockerror listener: {:?}", e))?;
        closure.forget();
    }

    // Mouse buttons
    {
        let state = state.clone();
        let closure = Closure::wrap(Box::new(move |event: MouseEvent| {
            let mut s = state.borrow_mut();
            s.mouse_buttons |= 1u8.checked_shl(event.button() as u32).unwrap_or(0);
        }) as Box<dyn FnMut(_)>);
        canvas
            .add_event_listener_with_callback("mousedown", closure.as_ref().unchecked_ref())
            .map_err(|e| format!("mousedown listener: {:?}", e))?;
        closure.forget();
    }

    {
        let state = state.clone();
        let closure = Closure::wrap(Box::new(move |event: MouseEvent| {
            let mut s = state.borrow_mut();
            s.mouse_buttons &= !(1u8.checked_shl(event.button() as u32).unwrap_or(0));
        }) as Box<dyn FnMut(_)>);
        canvas
            .add_event_listener_with_callback("mouseup", closure.as_ref().unchecked_ref())
            .map_err(|e| format!("mouseup listener: {:?}", e))?;
        closure.forget();
    }

    // Click canvas to request pointer lock (FPS-style mouse capture)
    {
        let canvas_clone = canvas.clone();
        let closure = Closure::wrap(Box::new(move |_: MouseEvent| {
            canvas_clone.request_pointer_lock();
        }) as Box<dyn FnMut(_)>);
        canvas
            .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
            .map_err(|e| format!("click listener: {:?}", e))?;
        closure.forget();
    }

    Ok(state)
}

