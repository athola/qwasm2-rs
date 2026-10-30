use super::fatal;
use q2_common::error::Q2Result;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Event, HtmlCanvasElement, KeyboardEvent, MouseEvent};

pub use crate::input_state::{ClickRequests, WasmInputState};
pub use crate::keymap::key_code_to_q2;

/// Set up keyboard and mouse event listeners on a canvas. With
/// `vid_fullscreen`, a click also takes the canvas's container fullscreen
/// and locks the keyboard to the page where the browser can (Keyboard Lock
/// API, Chromium only): there Ctrl+W, Ctrl+T and the like reach the game,
/// and Esc must be held to leave fullscreen.
/// Returns a shared reference to the input state.
pub fn setup_input_listeners(
    canvas: &HtmlCanvasElement,
    vid_fullscreen: bool,
) -> Q2Result<Rc<RefCell<WasmInputState>>> {
    let state = Rc::new(RefCell::new(WasmInputState::default()));
    let document = web_sys::window()
        .ok_or_else(|| fatal("no window"))?
        .document()
        .ok_or_else(|| fatal("no document"))?;

    // Keyboard down. Here and on keyup, only keys the engine maps are
    // swallowed, and only while the pointer is locked to the game, so browser
    // shortcuts (F5, Ctrl+F) and typing into other inputs keep working.
    {
        let state = state.clone();
        let lock_doc = document.clone();
        let closure = Closure::wrap(Box::new(move |event: KeyboardEvent| {
            if let Some(key) = key_code_to_q2(&event.code()) {
                state.borrow_mut().keys[key as usize] = true;
                if lock_doc.pointer_lock_element().is_some() {
                    event.prevent_default();
                }
            }
        }) as Box<dyn FnMut(_)>);
        document
            .add_event_listener_with_callback("keydown", closure.as_ref().unchecked_ref())
            .map_err(|e| fatal(format!("keydown listener: {:?}", e)))?;
        closure.forget(); // leak -- lives for lifetime of page
    }

    // Keyboard up
    {
        let state = state.clone();
        let lock_doc = document.clone();
        let closure = Closure::wrap(Box::new(move |event: KeyboardEvent| {
            if let Some(key) = key_code_to_q2(&event.code()) {
                state.borrow_mut().keys[key as usize] = false;
                if lock_doc.pointer_lock_element().is_some() {
                    event.prevent_default();
                }
            }
        }) as Box<dyn FnMut(_)>);
        document
            .add_event_listener_with_callback("keyup", closure.as_ref().unchecked_ref())
            .map_err(|e| fatal(format!("keyup listener: {:?}", e)))?;
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
            .map_err(|e| fatal(format!("mousemove listener: {:?}", e)))?;
        closure.forget();
    }

    // Pointer lock gained or lost (also fired again when the locked canvas
    // asks again): see `WasmInputState::pointer_lock_changed`.
    {
        let state = state.clone();
        let lock_doc = document.clone();
        let closure = Closure::wrap(Box::new(move |_: Event| {
            let locked = lock_doc.pointer_lock_element().is_some();
            state.borrow_mut().pointer_lock_changed(locked);
        }) as Box<dyn FnMut(_)>);
        document
            .add_event_listener_with_callback("pointerlockchange", closure.as_ref().unchecked_ref())
            .map_err(|e| fatal(format!("pointerlockchange listener: {:?}", e)))?;
        closure.forget();
    }

    // Losing focus (alt-tab, ctrl-tab) swallows the key-ups of held keys.
    {
        let state = state.clone();
        let closure = Closure::wrap(Box::new(move |_: Event| {
            state.borrow_mut().release_all();
        }) as Box<dyn FnMut(_)>);
        web_sys::window()
            .ok_or_else(|| fatal("no window"))?
            .add_event_listener_with_callback("blur", closure.as_ref().unchecked_ref())
            .map_err(|e| fatal(format!("blur listener: {:?}", e)))?;
        closure.forget();
    }

    // Log pointer lock errors to the browser console for diagnostics.
    {
        let closure = Closure::wrap(Box::new(move |_: Event| {
            web_sys::console::warn_1(
                &"[qwasm2] Pointer lock request failed (browser policy or missing user gesture)"
                    .into(),
            );
        }) as Box<dyn FnMut(_)>);
        document
            .add_event_listener_with_callback("pointerlockerror", closure.as_ref().unchecked_ref())
            .map_err(|e| fatal(format!("pointerlockerror listener: {:?}", e)))?;
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
            .map_err(|e| fatal(format!("mousedown listener: {:?}", e)))?;
        closure.forget();
    }

    {
        let state = state.clone();
        let closure = Closure::wrap(Box::new(move |event: MouseEvent| {
            let mut s = state.borrow_mut();
            s.mouse_buttons &= !(1u8.checked_shl(event.button() as u32).unwrap_or(0));
        }) as Box<dyn FnMut(_)>);
        // On the document: a button pressed on the canvas may be released
        // after the cursor has left it.
        document
            .add_event_listener_with_callback("mouseup", closure.as_ref().unchecked_ref())
            .map_err(|e| fatal(format!("mouseup listener: {:?}", e)))?;
        closure.forget();
    }

    // Click canvas to request pointer lock (FPS-style mouse capture). Current
    // browsers return a Promise that rejects when the lock is refused;
    // web-sys's binding drops it, which surfaces as an uncaught rejection, so
    // call it through JS and log the refusal instead.
    {
        let canvas_clone = canvas.clone();
        let lock_doc = document.clone();
        let on_refused = Closure::wrap(Box::new(move |err: JsValue| {
            let reason = js_sys::Reflect::get(&err, &"message".into())
                .ok()
                .and_then(|m| m.as_string())
                .unwrap_or_default();
            web_sys::console::warn_1(&format!("[qwasm2] pointer lock refused: {reason}").into());
        }) as Box<dyn FnMut(JsValue)>);
        let closure = Closure::wrap(Box::new(move |_: MouseEvent| {
            // Every mouse shot is a click: ask only for what is missing.
            let element: &web_sys::Element = canvas_clone.as_ref();
            let requests = ClickRequests::for_click(
                vid_fullscreen,
                lock_doc.fullscreen_element().is_some(),
                lock_doc.pointer_lock_element().as_ref() == Some(element),
            );
            if requests.fullscreen {
                enter_fullscreen(&lock_doc, element);
            }
            if !requests.pointer_lock {
                return;
            }
            let request = js_sys::Reflect::get(&canvas_clone, &"requestPointerLock".into())
                .ok()
                .and_then(|f| f.dyn_into::<js_sys::Function>().ok());
            if let Some(request) = request {
                match request.call0(&canvas_clone) {
                    Ok(result) => {
                        if let Ok(promise) = result.dyn_into::<js_sys::Promise>() {
                            let _ = promise.catch(&on_refused);
                        }
                    }
                    Err(e) => web_sys::console::warn_1(&e),
                }
            }
        }) as Box<dyn FnMut(_)>);
        canvas
            .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
            .map_err(|e| fatal(format!("click listener: {:?}", e)))?;
        closure.forget();
    }

    Ok(state)
}

/// Take `#game-container` (the canvas and its overlays; else the canvas)
/// fullscreen, then lock the keyboard to the page where the browser has
/// `navigator.keyboard.lock` (Chromium). Refusals are logged.
fn enter_fullscreen(document: &web_sys::Document, canvas: &web_sys::Element) {
    let target = document
        .get_element_by_id("game-container")
        .unwrap_or_else(|| canvas.clone());
    if let Err(e) = target.request_fullscreen() {
        web_sys::console::warn_1(&e);
        return;
    }
    let Some(window) = web_sys::window() else {
        return;
    };
    let keyboard = js_sys::Reflect::get(&window, &"navigator".into())
        .and_then(|navigator| js_sys::Reflect::get(&navigator, &"keyboard".into()))
        .ok()
        .filter(|k| !k.is_undefined());
    let lock = keyboard.as_ref().and_then(|k| {
        js_sys::Reflect::get(k, &"lock".into())
            .ok()
            .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
    });
    match (keyboard, lock) {
        (Some(keyboard), Some(lock)) => match lock.call0(&keyboard) {
            Ok(result) => {
                if let Ok(promise) = result.dyn_into::<js_sys::Promise>() {
                    // Once per entry into fullscreen, so leaking it is small.
                    let refused = Closure::wrap(Box::new(|err: JsValue| {
                        web_sys::console::warn_2(&"[qwasm2] keyboard lock refused:".into(), &err);
                    }) as Box<dyn FnMut(JsValue)>);
                    let _ = promise.catch(&refused);
                    refused.forget();
                }
            }
            Err(e) => web_sys::console::warn_1(&e),
        },
        _ => web_sys::console::warn_1(
            &"[qwasm2] no Keyboard Lock API: browser shortcuts like Ctrl+W stay the browser's"
                .into(),
        ),
    }
}
