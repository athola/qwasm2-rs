use super::fatal;
use q2_common::error::Q2Result;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::convert::FromWasmAbi;
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

    // Keyboard down and up. Only keys the engine maps are swallowed, and
    // only while the pointer is locked to the game, so browser shortcuts
    // (F5, Ctrl+F) and typing into other inputs keep working.
    key_listener(&document, &state, "keydown", WasmInputState::key_down)?;
    key_listener(&document, &state, "keyup", WasmInputState::key_up)?;

    // Mouse move — listen on document so events are received regardless of cursor position.
    // Only accumulate deltas while pointer lock is active; without lock, movementX/Y can
    // produce a large spurious jump when the cursor re-enters the viewport boundary.
    {
        let state = state.clone();
        let document_for_move = document.clone();
        listen(&document, "mousemove", move |event: MouseEvent| {
            if document_for_move.pointer_lock_element().is_some() {
                let mut s = state.borrow_mut();
                s.mouse_dx += event.movement_x() as f32;
                s.mouse_dy += event.movement_y() as f32;
            }
        })?;
    }

    // Pointer lock gained or lost (also fired again when the locked canvas
    // asks again): see `WasmInputState::pointer_lock_changed`.
    {
        let state = state.clone();
        let lock_doc = document.clone();
        listen(&document, "pointerlockchange", move |_: Event| {
            let locked = lock_doc.pointer_lock_element().is_some();
            state.borrow_mut().pointer_lock_changed(locked);
        })?;
    }

    // Losing focus (alt-tab, ctrl-tab) swallows the key-ups of held keys.
    {
        let state = state.clone();
        let window = web_sys::window().ok_or_else(|| fatal("no window"))?;
        listen(&window, "blur", move |_: Event| {
            state.borrow_mut().release_all()
        })?;
    }

    // The page's Keys button (`qwasm2-legend`), read once by the next frame.
    {
        let state = state.clone();
        listen(&document, "qwasm2-legend", move |_: Event| {
            state.borrow_mut().legend_requested = true;
        })?;
    }

    // Log pointer lock errors to the browser console for diagnostics.
    listen(&document, "pointerlockerror", move |_: Event| {
        web_sys::console::warn_1(
            &"[qwasm2] Pointer lock request failed (browser policy or missing user gesture)".into(),
        );
    })?;

    // Mouse buttons
    {
        let state = state.clone();
        listen(canvas, "mousedown", move |event: MouseEvent| {
            let mask = 1u8.checked_shl(event.button() as u32).unwrap_or(0);
            state.borrow_mut().button_down(mask);
        })?;
    }

    // On the document: a button pressed on the canvas may be released
    // after the cursor has left it.
    {
        let state = state.clone();
        listen(&document, "mouseup", move |event: MouseEvent| {
            let mask = 1u8.checked_shl(event.button() as u32).unwrap_or(0);
            state.borrow_mut().button_up(mask);
        })?;
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
        listen(canvas, "click", move |_: MouseEvent| {
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
            if let Some(request) = js_method(&canvas_clone, "requestPointerLock") {
                match request.call0(&canvas_clone) {
                    Ok(result) => {
                        if let Ok(promise) = result.dyn_into::<js_sys::Promise>() {
                            let _ = promise.catch(&on_refused);
                        }
                    }
                    Err(e) => web_sys::console::warn_1(&e),
                }
            }
        })?;
    }

    Ok(state)
}

/// Add `handler` as the `name` listener of `target`, for the life of the
/// page (the closure is leaked).
fn listen<E: FromWasmAbi + 'static>(
    target: &web_sys::EventTarget,
    name: &str,
    handler: impl FnMut(E) + 'static,
) -> Q2Result<()> {
    let closure = Closure::wrap(Box::new(handler) as Box<dyn FnMut(E)>);
    target
        .add_event_listener_with_callback(name, closure.as_ref().unchecked_ref())
        .map_err(|e| fatal(format!("{name} listener: {e:?}")))?;
    closure.forget();
    Ok(())
}

/// A key event listener on the document that applies mapped keys to the
/// input state, and swallows them while the pointer is locked.
fn key_listener(
    document: &web_sys::Document,
    state: &Rc<RefCell<WasmInputState>>,
    name: &str,
    apply: fn(&mut WasmInputState, u8),
) -> Q2Result<()> {
    let state = state.clone();
    let lock_doc = document.clone();
    listen(document, name, move |event: KeyboardEvent| {
        if let Some(key) = key_code_to_q2(&event.code()) {
            apply(&mut state.borrow_mut(), key);
            if lock_doc.pointer_lock_element().is_some() {
                event.prevent_default();
            }
        }
    })
}

/// `obj[name]`, when it is a function. Called through Reflect for the
/// Promises web_sys's bindings drop.
fn js_method(obj: &JsValue, name: &str) -> Option<js_sys::Function> {
    js_sys::Reflect::get(obj, &name.into())
        .ok()?
        .dyn_into()
        .ok()
}

/// Take `#game-container` (the canvas and its overlays; else the canvas)
/// fullscreen, then, once the browser grants it, lock the keyboard to the
/// page where the browser has `navigator.keyboard.lock` (Chromium).
/// Refusals are logged.
fn enter_fullscreen(document: &web_sys::Document, canvas: &web_sys::Element) {
    let target = document
        .get_element_by_id("game-container")
        .unwrap_or_else(|| canvas.clone());
    // Called through Reflect for the Promise web_sys's binding drops: a
    // refusal rejects it rather than throwing.
    let Some(request) = js_method(&target, "requestFullscreen") else {
        web_sys::console::warn_1(&"[qwasm2] no Fullscreen API".into());
        return;
    };
    match request.call0(&target) {
        Ok(result) => match result.dyn_into::<js_sys::Promise>() {
            Ok(promise) => {
                // Once per entry into fullscreen, so leaking these is small.
                let granted = Closure::wrap(
                    Box::new(|_: JsValue| lock_keyboard()) as Box<dyn FnMut(JsValue)>
                );
                let refused = Closure::wrap(Box::new(|err: JsValue| {
                    web_sys::console::warn_2(&"[qwasm2] fullscreen refused:".into(), &err);
                }) as Box<dyn FnMut(JsValue)>);
                let _ = promise.then2(&granted, &refused);
                granted.forget();
                refused.forget();
            }
            // Browsers from before the promise-returning API.
            Err(_) => lock_keyboard(),
        },
        Err(e) => web_sys::console::warn_2(&"[qwasm2] fullscreen refused:".into(), &e),
    }
}

/// Lock the keyboard to the page (`navigator.keyboard.lock`), so browser
/// shortcuts reach the game in fullscreen. Refusals are logged.
fn lock_keyboard() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let keyboard = js_sys::Reflect::get(&window, &"navigator".into())
        .and_then(|navigator| js_sys::Reflect::get(&navigator, &"keyboard".into()))
        .ok()
        .filter(|k| !k.is_undefined());
    let lock = keyboard.as_ref().and_then(|k| js_method(k, "lock"));
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
