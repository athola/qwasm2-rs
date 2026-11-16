//! WebAudio output for the client sound system (the platform half of C's
//! SDL sound backend, client/sound/sdl.c).
//!
//! Each voice is an `AudioBufferSourceNode` feeding a left and a right
//! `GainNode` into the two inputs of one `ChannelMergerNode`, so the
//! client's separate left/right volumes reach the speakers as C mixes
//! them. Buffers are built from samples the client decoded itself, keeping
//! the WAV `cue ` loop points that `decodeAudioData` would drop.

use std::collections::HashMap;

use super::fatal;
use q2_common::error::Q2Result;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{
    AudioBuffer, AudioBufferSourceNode, AudioContext, AudioContextState, AudioScheduledSourceNode,
    ChannelMergerNode, GainNode, HtmlCanvasElement,
};

/// Decoded mono samples to play (the client's `wav::Sound`).
#[derive(Debug, Clone, Copy)]
pub struct Pcm<'a> {
    pub samples: &'a [f32],
    pub rate: u32,
    /// Sample a one-shot loops back to at its end.
    pub loop_start: Option<usize>,
}

struct Voice {
    source: AudioBufferSourceNode,
    left: GainNode,
    right: GainNode,
}

impl Voice {
    fn set_gain(&self, left: f32, right: f32) {
        self.left.gain().set_value(left);
        self.right.gain().set_value(right);
    }

    /// Stop the source and detach its gains from the output.
    fn stop(self) {
        if let Err(e) = AudioScheduledSourceNode::stop(&self.source) {
            web_sys::console::warn_1(&e);
        }
        for gain in [&self.left, &self.right] {
            if let Err(e) = gain.disconnect() {
                web_sys::console::warn_1(&e);
            }
        }
    }
}

pub struct WebAudio {
    ctx: AudioContext,
    out: ChannelMergerNode,
    buffers: HashMap<usize, AudioBuffer>,
    /// One-shot voices by client channel.
    voices: HashMap<usize, Voice>,
    /// Entity loop voices by sound.
    loops: HashMap<usize, Voice>,
}

impl WebAudio {
    /// Create the context and resume it on each click of `canvas`: browsers
    /// keep a context suspended until a user gesture, and the canvas click
    /// that takes the pointer lock is the page's "Click to play".
    /// `on_state` hears the first state and every change (`suspended`,
    /// `running`).
    pub fn new(canvas: &HtmlCanvasElement, on_state: Box<dyn Fn(&str)>) -> Q2Result<Self> {
        let err = |what: &str, e: JsValue| fatal(format!("{what}: {e:?}"));
        let ctx = AudioContext::new().map_err(|e| err("AudioContext", e))?;
        let out = ctx
            .create_channel_merger_with_number_of_inputs(2)
            .map_err(|e| err("channel merger", e))?;
        out.connect_with_audio_node(&ctx.destination())
            .map_err(|e| err("connect output", e))?;

        on_state(state_name(ctx.state()));
        let state_ctx = ctx.clone();
        let on_change = Closure::wrap(Box::new(move || {
            on_state(state_name(state_ctx.state()));
        }) as Box<dyn Fn()>);
        ctx.set_onstatechange(Some(on_change.as_ref().unchecked_ref()));
        on_change.forget(); // lives for the page

        let resume_ctx = ctx.clone();
        let on_refused = Closure::wrap(Box::new(move |e: JsValue| {
            web_sys::console::warn_1(&e);
        }) as Box<dyn FnMut(JsValue)>);
        let on_click = Closure::wrap(Box::new(move || {
            if resume_ctx.state() == AudioContextState::Running {
                return;
            }
            match resume_ctx.resume() {
                Ok(promise) => {
                    let _ = promise.catch(&on_refused);
                }
                Err(e) => web_sys::console::warn_1(&e),
            }
        }) as Box<dyn FnMut()>);
        canvas
            .add_event_listener_with_callback("click", on_click.as_ref().unchecked_ref())
            .map_err(|e| err("audio click listener", e))?;
        on_click.forget();

        Ok(Self {
            ctx,
            out,
            buffers: HashMap::new(),
            voices: HashMap::new(),
            loops: HashMap::new(),
        })
    }

    /// Whether sound plays: a suspended context would queue every sound
    /// and start them together when resumed.
    pub fn running(&self) -> bool {
        self.ctx.state() == AudioContextState::Running
    }

    /// The context's clock, which voices are scheduled on.
    pub fn now_ms(&self) -> f64 {
        self.ctx.current_time() * 1000.0
    }

    fn buffer(&mut self, id: usize, pcm: Pcm) -> Result<AudioBuffer, JsValue> {
        if let Some(buffer) = self.buffers.get(&id) {
            return Ok(buffer.clone());
        }
        let length = u32::try_from(pcm.samples.len()).map_err(|_| JsValue::from_str("too long"))?;
        let buffer = self.ctx.create_buffer(1, length, pcm.rate as f32)?;
        buffer.copy_to_channel(pcm.samples, 0)?;
        self.buffers.insert(id, buffer.clone());
        Ok(buffer)
    }

    fn voice(&mut self, id: usize, pcm: Pcm, left: f32, right: f32) -> Result<Voice, JsValue> {
        let buffer = self.buffer(id, pcm)?;
        let source = self.ctx.create_buffer_source()?;
        source.set_buffer(Some(&buffer));
        let (left_gain, right_gain) = (self.ctx.create_gain()?, self.ctx.create_gain()?);
        source.connect_with_audio_node(&left_gain)?;
        source.connect_with_audio_node(&right_gain)?;
        left_gain.connect_with_audio_node_and_output_and_input(&self.out, 0, 0)?;
        right_gain.connect_with_audio_node_and_output_and_input(&self.out, 0, 1)?;
        let voice = Voice {
            source,
            left: left_gain,
            right: right_gain,
        };
        voice.set_gain(left, right);
        Ok(voice)
    }

    /// Start `pcm` on `channel` after `delay` seconds, replacing what the
    /// channel played. A sound with a loop point loops from it, as C's
    /// mixer does.
    pub fn play(&mut self, channel: usize, id: usize, pcm: Pcm, left: f32, right: f32, delay: f32) {
        self.stop(channel);
        let started = self.voice(id, pcm, left, right).and_then(|voice| {
            if let Some(start) = pcm.loop_start {
                voice.source.set_loop(true);
                voice
                    .source
                    .set_loop_start(start as f64 / f64::from(pcm.rate));
                voice
                    .source
                    .set_loop_end(pcm.samples.len() as f64 / f64::from(pcm.rate));
            }
            let when = self.ctx.current_time() + f64::from(delay);
            AudioScheduledSourceNode::start_with_when(&voice.source, when)?;
            Ok(voice)
        });
        match started {
            Ok(voice) => {
                self.voices.insert(channel, voice);
            }
            Err(e) => web_sys::console::warn_1(&e),
        }
    }

    pub fn set_gain(&mut self, channel: usize, left: f32, right: f32) {
        if let Some(voice) = self.voices.get(&channel) {
            voice.set_gain(left, right);
        }
    }

    pub fn stop(&mut self, channel: usize) {
        if let Some(voice) = self.voices.remove(&channel) {
            voice.stop();
        }
    }

    /// Play exactly these loops at these gains: new ones start in step
    /// with the context clock (C `paintedtime % length`), missing ones
    /// stop.
    pub fn set_loops(&mut self, loops: &[(usize, Pcm, f32, f32)]) {
        let stale: Vec<usize> = self
            .loops
            .keys()
            .filter(|id| !loops.iter().any(|(l, ..)| l == *id))
            .copied()
            .collect();
        for id in stale {
            if let Some(voice) = self.loops.remove(&id) {
                voice.stop();
            }
        }
        for &(id, pcm, left, right) in loops {
            if let Some(voice) = self.loops.get(&id) {
                voice.set_gain(left, right);
                continue;
            }
            let started = self.voice(id, pcm, left, right).and_then(|voice| {
                voice.source.set_loop(true);
                let length = pcm.samples.len() as f64 / f64::from(pcm.rate);
                let now = self.ctx.current_time();
                voice
                    .source
                    .start_with_when_and_grain_offset(now, now % length.max(f64::EPSILON))?;
                Ok(voice)
            });
            match started {
                Ok(voice) => {
                    self.loops.insert(id, voice);
                }
                Err(e) => web_sys::console::warn_1(&e),
            }
        }
    }
}

fn state_name(state: AudioContextState) -> &'static str {
    match state {
        AudioContextState::Suspended => "suspended",
        AudioContextState::Running => "running",
        AudioContextState::Closed => "closed",
        _ => "unknown",
    }
}
