//! Client sound system (C client/sound/sound.c with the SDL backend's
//! mixing decisions, client/sound/sdl.c).
//!
//! `SoundSystem` keeps C's 32 channels, picks and overrides them, and
//! spatializes every sound with the SDL backend's integer left/right
//! volumes. It does not mix: each `update` returns what a platform mixer
//! must start, re-gain or stop, plus the merged entity loop sounds, so the
//! decisions are testable without an audio device. Sound starts are issued
//! at the next `update` with their `timeofs` as a start delay, where C
//! drifts them against the server clock (`SDL_DriftBeginofs`).

pub mod wav;

use std::rc::Rc;

use q2_shared::constants::{CS_PLAYERSKINS, MAX_SOUNDS};
use q2_shared::types::{EntityState, Vec3f};

use wav::Sound;

/// C `MAX_CHANNELS`.
pub const MAX_CHANNELS: usize = 32;
/// C `ATTN_STATIC`: attenuates twice as fast as the other attenuations.
pub const ATTN_STATIC: f32 = 3.0;
/// C `SDL_FULLVOLUME`: closer than this is full volume.
const FULL_VOLUME_DIST: f32 = 80.0;
/// C `SDL_LOOPATTENUATE`: the distance multiplier of entity loop sounds.
const LOOP_ATTENUATE: f32 = 0.003;
/// C `(int)(s_volume->value * 256)` with `s_volume`'s default of 0.7.
const SND_VOL: i32 = 179;

/// Index into the system's known sounds (C `known_sfx`).
pub type SfxId = usize;

/// Where the sounds are heard from (C `listener_origin`, `listener_right`;
/// the SDL backend reads no other axis).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Listener {
    pub origin: Vec3f,
    pub right: Vec3f,
}

impl Listener {
    /// The listener at the view (C `S_Update(cl.refdef.vieworg, v_forward,
    /// v_right, v_up)`), `right` from C `AngleVectors`.
    pub fn from_view(origin: Vec3f, angles: Vec3f) -> Self {
        let rad = |degrees: f32| f64::from(degrees) * (std::f64::consts::PI / 180.0);
        let (sp, cp) = (rad(angles.x).sin() as f32, rad(angles.x).cos() as f32);
        let (sy, cy) = (rad(angles.y).sin() as f32, rad(angles.y).cos() as f32);
        let (sr, cr) = (rad(angles.z).sin() as f32, rad(angles.z).cos() as f32);
        let right = Vec3f::new(
            -(sr * sp * cy) + (cr * sy),
            -(sr * sp * sy) + -(cr * cy),
            -sr * cp,
        );
        Self { origin, right }
    }
}

/// C `SDL_SpatializeOrigin`: left and right volumes 0..=`master_vol` for a
/// sound at `origin`, by distance past `FULL_VOLUME_DIST` times
/// `dist_mult` and by which side of the listener it is on. A `dist_mult`
/// of 0 (`ATTN_NONE`) is heard at full volume in both ears everywhere.
pub fn spatialize_origin(
    origin: Vec3f,
    listener: &Listener,
    master_vol: f32,
    dist_mult: f32,
) -> (i32, i32) {
    let source = origin - listener.origin;
    let length = source.length();
    let source = if length > 0.0 {
        source / length
    } else {
        Vec3f::ZERO
    };
    let dist = (length - FULL_VOLUME_DIST).max(0.0) * dist_mult;
    let dot = listener.right.dot(source);
    let (rscale, lscale) = if dist_mult == 0.0 {
        (1.0, 1.0)
    } else {
        (0.5 * (1.0 + dot), 0.5 * (1.0 - dot))
    };
    let right = ((master_vol * ((1.0 - dist) * rscale)) as i32).max(0);
    let left = ((master_vol * ((1.0 - dist) * lscale)) as i32).max(0);
    (left, right)
}

/// C `S_IssuePlaysound`'s distance multiplier for an attenuation.
pub fn dist_mult(attenuation: f32) -> f32 {
    if attenuation == ATTN_STATIC {
        attenuation * 0.001
    } else {
        attenuation * 0.0005
    }
}

/// The output gain of a 0..=255 channel volume: C's mixer scales samples
/// by `vol * snd_vol >> 8` and the paint buffer by `>> 8` again.
pub fn gain(vol: i32) -> f32 {
    (vol * SND_VOL) as f32 / 65536.0
}

/// C `CL_GetEntitySoundOrigin`: entity `number`'s origin lerped from the
/// previous frame to the current one (frames sorted by number, as
/// `FrameHistory` keeps them); an entity new this frame is not lerped.
pub fn entity_origin(
    current: &[EntityState],
    previous: &[EntityState],
    lerpfrac: f32,
    number: i32,
) -> Option<Vec3f> {
    let find = |frame: &[EntityState]| {
        frame
            .binary_search_by_key(&number, |s| s.number)
            .ok()
            .map(|i| frame[i].origin)
    };
    let now = find(current)?;
    let before = find(previous).unwrap_or(now);
    Some(before + (now - before) * lerpfrac)
}

/// A sound the server started (C `CL_ParseStartSoundPacket`'s fields).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StartSound {
    pub entnum: i32,
    pub entchannel: i32,
    /// `CS_SOUNDS` index.
    pub sound_index: i32,
    pub volume: f32,
    pub attenuation: f32,
    pub timeofs: f32,
    /// A fixed origin, or `None` to follow the entity.
    pub origin: Option<Vec3f>,
}

/// What the platform mixer does with one channel; gains come from `gain`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MixCommand {
    /// Start `sfx` on `channel` after `delay` seconds, replacing whatever
    /// the channel played.
    Play {
        channel: usize,
        sfx: SfxId,
        left: f32,
        right: f32,
        delay: f32,
    },
    Gain {
        channel: usize,
        left: f32,
        right: f32,
    },
    Stop {
        channel: usize,
    },
}

/// One looped sound this frame, all entities playing it merged (C
/// `SDL_AddLoopSounds`). C restarts loops from `paintedtime % length`
/// each frame, so the mixer keeps them in step with its clock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoopMix {
    pub sfx: SfxId,
    pub left: f32,
    pub right: f32,
}

/// What `update` asks the mixer to do this frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mix {
    pub commands: Vec<MixCommand>,
    /// Every loop that should be heard; any other loop stops.
    pub loops: Vec<LoopMix>,
}

/// C `sfx_t`.
#[derive(Debug, Clone)]
struct Sfx {
    name: String,
    /// The file an alias loads (C `truename`).
    truename: Option<String>,
    sound: Option<Rc<Sound>>,
    /// Whether loading was tried, so a missing file is not reread.
    tried: bool,
}

/// C `channel_t`, with time in milliseconds of the caller's clock where C
/// counts painted samples.
#[derive(Debug, Clone, Copy, Default)]
struct Channel {
    sfx: Option<SfxId>,
    leftvol: i32,
    rightvol: i32,
    end_ms: f64,
    entnum: i32,
    entchannel: i32,
    origin: Vec3f,
    dist_mult: f32,
    master_vol: i32,
    fixed_origin: bool,
    autosound: bool,
}

/// A started sound waiting for the next update (C `playsound_t`).
#[derive(Debug, Clone, Copy)]
struct PlaySound {
    sfx: SfxId,
    entnum: i32,
    entchannel: i32,
    origin: Option<Vec3f>,
    volume: f32,
    attenuation: f32,
    timeofs: f32,
}

#[derive(Debug)]
pub struct SoundSystem {
    known: Vec<Sfx>,
    /// C `cl.sound_precache`, by `CS_SOUNDS` index.
    precache: Vec<Option<SfxId>>,
    channels: [Channel; MAX_CHANNELS],
    pending: Vec<PlaySound>,
    /// The local player's entity number (C `cl.playernum + 1`).
    view_entity: i32,
    /// Stops queued outside `update`.
    queued: Vec<MixCommand>,
}

impl SoundSystem {
    pub fn new(view_entity: i32) -> Self {
        Self {
            known: Vec::new(),
            precache: vec![None; MAX_SOUNDS],
            channels: [Channel::default(); MAX_CHANNELS],
            pending: Vec::new(),
            view_entity,
            queued: Vec::new(),
        }
    }

    /// The decoded samples of `sfx`, once loaded.
    pub fn sound(&self, sfx: SfxId) -> Option<&Rc<Sound>> {
        self.known.get(sfx)?.sound.as_ref()
    }

    /// The name `sfx` was registered under.
    pub fn name(&self, sfx: SfxId) -> &str {
        self.known.get(sfx).map_or("", |s| s.name.as_str())
    }

    /// C `S_FindName` with `create`.
    fn find_or_add(&mut self, name: &str) -> SfxId {
        if let Some(id) = self.known.iter().position(|s| s.name == name) {
            return id;
        }
        self.known.push(Sfx {
            name: name.to_string(),
            truename: None,
            sound: None,
            tried: false,
        });
        self.known.len() - 1
    }

    /// C `S_LoadSound`: `#name` is a path from the game root, anything
    /// else is under `sound/`; `*` names are placeholders for the sexed
    /// sound and never load.
    fn load(&mut self, id: SfxId, load: &mut dyn FnMut(&str) -> Option<Vec<u8>>) -> bool {
        let sfx = &mut self.known[id];
        if sfx.name.starts_with('*') {
            return false;
        }
        if !sfx.tried {
            sfx.tried = true;
            let name = sfx.truename.as_deref().unwrap_or(&sfx.name);
            let path = match name.strip_prefix('#') {
                Some(path) => path.to_string(),
                None => format!("sound/{name}"),
            };
            sfx.sound = match load(&path).map(|bytes| wav::decode(&path, &bytes)) {
                Some(Ok(sound)) => Some(Rc::new(sound)),
                Some(Err(e)) => {
                    tracing::warn!("{e}");
                    None
                }
                None => {
                    tracing::debug!("Couldn't load {path}");
                    None
                }
            };
        }
        sfx.sound.is_some()
    }

    /// C `CL_RegisterSounds`: every `CS_SOUNDS` name up to the first empty
    /// one, loaded now (C `S_EndRegistration`). Sounds stay known across
    /// maps; C frees the ones a map stopped using only when its table runs
    /// short.
    pub fn register_sounds<'a>(
        &mut self,
        configstring: impl Fn(usize) -> &'a str,
        mut load: impl FnMut(&str) -> Option<Vec<u8>>,
    ) {
        use q2_shared::constants::CS_SOUNDS;
        self.precache.fill(None);
        for i in 1..MAX_SOUNDS {
            let name = configstring(CS_SOUNDS + i);
            if name.is_empty() {
                break;
            }
            let id = self.find_or_add(name);
            self.load(id, &mut load);
            self.precache[i] = Some(id);
        }
    }

    /// C `S_RegisterSexedSound`: `*name` played by `entnum` is
    /// `players/<model>/name` for the model in its `CS_PLAYERSKINS`
    /// string (`name\model/skin`, male if none), or the male sound when
    /// that model has no such file.
    fn sexed_sound<'a>(
        &mut self,
        entnum: i32,
        base: &str,
        configstring: &impl Fn(usize) -> &'a str,
        load: &mut dyn FnMut(&str) -> Option<Vec<u8>>,
    ) -> SfxId {
        let skin = usize::try_from(entnum - 1)
            .map(|n| configstring(CS_PLAYERSKINS + n))
            .unwrap_or("");
        let model = skin
            .split_once('\\')
            .map(|(_, rest)| rest.split('/').next().unwrap_or(""))
            .filter(|m| !m.is_empty())
            .unwrap_or("male");
        let file = &base[1..];
        let sexed = format!("#players/{model}/{file}");
        if let Some(id) = self.known.iter().position(|s| s.name == sexed) {
            return id;
        }
        let id = self.find_or_add(&sexed);
        if !self.load(id, load) {
            // Not in the model's directory: an alias of the male sound in
            // pak0 (C S_AliasName).
            let sfx = &mut self.known[id];
            sfx.truename = Some(format!("player/male/{file}"));
            sfx.tried = false;
        }
        id
    }

    /// C `CL_ParseStartSoundPacket` -> `S_StartSound`: queue the sound
    /// for the next update. Unregistered or unloadable sounds are dropped.
    pub fn start_sound<'a>(
        &mut self,
        start: &StartSound,
        configstring: impl Fn(usize) -> &'a str,
        mut load: impl FnMut(&str) -> Option<Vec<u8>>,
    ) {
        let Some(mut id) = usize::try_from(start.sound_index)
            .ok()
            .and_then(|i| self.precache.get(i).copied().flatten())
        else {
            return;
        };
        if self.known[id].name.starts_with('*') {
            let base = self.known[id].name.clone();
            id = self.sexed_sound(start.entnum, &base, &configstring, &mut load);
        }
        if !self.load(id, &mut load) {
            return;
        }
        self.pending.push(PlaySound {
            sfx: id,
            entnum: start.entnum,
            entchannel: start.entchannel,
            origin: start.origin,
            volume: start.volume,
            attenuation: start.attenuation,
            timeofs: start.timeofs,
        });
    }

    /// C `S_StopAllSounds`: silence every channel and drop pending sounds.
    pub fn stop_all(&mut self) {
        for (i, ch) in self.channels.iter_mut().enumerate() {
            if ch.sfx.is_some() && !ch.autosound {
                self.queued.push(MixCommand::Stop { channel: i });
            }
            *ch = Channel::default();
        }
        self.pending.clear();
    }

    /// C `S_PickChannel`: the channel this entity already plays on
    /// `entchannel` (never for channel 0), else the one closest to its end
    /// that is not a player sound wanted by another entity. A one-shot
    /// still playing there is stopped (C clears the channel).
    fn pick_channel(
        &mut self,
        entnum: i32,
        entchannel: i32,
        now_ms: f64,
        commands: &mut Vec<MixCommand>,
    ) -> Option<usize> {
        let mut first_to_die = None;
        let mut life_left = f64::MAX;
        for (i, ch) in self.channels.iter().enumerate() {
            if entchannel != 0 && ch.entnum == entnum && ch.entchannel == entchannel {
                first_to_die = Some(i);
                break;
            }
            // Don't let monster sounds override player sounds.
            if ch.entnum == self.view_entity && entnum != self.view_entity && ch.sfx.is_some() {
                continue;
            }
            let left = if ch.sfx.is_some() {
                ch.end_ms - now_ms
            } else {
                f64::MIN
            };
            if left < life_left {
                life_left = left;
                first_to_die = Some(i);
            }
        }
        let i = first_to_die?;
        if self.channels[i].sfx.is_some() && !self.channels[i].autosound {
            commands.push(MixCommand::Stop { channel: i });
        }
        self.channels[i] = Channel::default();
        Some(i)
    }

    /// C `SDL_Spatialize`: the view entity's own sounds are full volume.
    fn spatialize(&self, ch: &mut Channel, listener: &Listener) {
        if ch.entnum == self.view_entity {
            (ch.leftvol, ch.rightvol) = (ch.master_vol, ch.master_vol);
            return;
        }
        (ch.leftvol, ch.rightvol) =
            spatialize_origin(ch.origin, listener, ch.master_vol as f32, ch.dist_mult);
    }

    /// One frame of C `SDL_Update` at `now_ms` on the mixer's clock:
    /// drop finished and inaudible channels, respatialize the rest from
    /// `entity_origin` (C `CL_GetEntitySoundOrigin`, the entity's lerped
    /// origin) unless their origin is fixed, merge the loop sounds of
    /// `entities` (the current frame), then issue the sounds started since
    /// the last update.
    pub fn update(
        &mut self,
        now_ms: f64,
        listener: &Listener,
        entity_origin: impl Fn(i32) -> Option<Vec3f>,
        entities: &[EntityState],
    ) -> Mix {
        let mut mix = Mix {
            commands: std::mem::take(&mut self.queued),
            loops: Vec::new(),
        };
        for i in 0..MAX_CHANNELS {
            let mut ch = self.channels[i];
            let Some(sfx) = ch.sfx else {
                continue;
            };
            if ch.autosound {
                self.channels[i] = Channel::default();
                continue;
            }
            if now_ms >= ch.end_ms {
                // C SDL_PaintChannels: a sound with a loop point goes back
                // to it, any other channel stops.
                let looped = self.sound(sfx).and_then(|s| {
                    let start = s.loop_start?;
                    Some((s.samples.len() - start) as f64 * 1000.0 / f64::from(s.rate))
                });
                match looped.filter(|&len| len > 0.0) {
                    Some(len) => {
                        while ch.end_ms <= now_ms {
                            ch.end_ms += len;
                        }
                    }
                    None => {
                        self.channels[i] = Channel::default();
                        mix.commands.push(MixCommand::Stop { channel: i });
                        continue;
                    }
                }
            }
            if !ch.fixed_origin {
                if let Some(origin) = entity_origin(ch.entnum) {
                    ch.origin = origin;
                }
            }
            self.spatialize(&mut ch, listener);
            if ch.leftvol == 0 && ch.rightvol == 0 {
                self.channels[i] = Channel::default();
                mix.commands.push(MixCommand::Stop { channel: i });
                continue;
            }
            self.channels[i] = ch;
            mix.commands.push(MixCommand::Gain {
                channel: i,
                left: gain(ch.leftvol),
                right: gain(ch.rightvol),
            });
        }

        self.add_loop_sounds(now_ms, listener, entities, &mut mix);

        for ps in std::mem::take(&mut self.pending) {
            let Some(i) = self.pick_channel(ps.entnum, ps.entchannel, now_ms, &mut mix.commands)
            else {
                continue;
            };
            let length_ms = self.sound(ps.sfx).map_or(0.0, |s| s.duration_ms());
            let mut ch = Channel {
                sfx: Some(ps.sfx),
                entnum: ps.entnum,
                entchannel: ps.entchannel,
                origin: ps
                    .origin
                    .unwrap_or_else(|| entity_origin(ps.entnum).unwrap_or_default()),
                fixed_origin: ps.origin.is_some(),
                dist_mult: dist_mult(ps.attenuation),
                master_vol: (ps.volume * 255.0) as i32,
                end_ms: now_ms + f64::from(ps.timeofs) * 1000.0 + length_ms,
                ..Channel::default()
            };
            self.spatialize(&mut ch, listener);
            self.channels[i] = ch;
            mix.commands.push(MixCommand::Play {
                channel: i,
                sfx: ps.sfx,
                left: gain(ch.leftvol),
                right: gain(ch.rightvol),
                delay: ps.timeofs,
            });
        }
        mix
    }

    /// C `SDL_AddLoopSounds`: every entity whose `sound` is set plays it in
    /// a loop; entities playing the same sound are summed into one channel
    /// (each side capped at 255), and an inaudible total is skipped.
    fn add_loop_sounds(
        &mut self,
        now_ms: f64,
        listener: &Listener,
        entities: &[EntityState],
        mix: &mut Mix,
    ) {
        let mut done = vec![false; entities.len()];
        for (i, ent) in entities.iter().enumerate() {
            if done[i] || ent.sound == 0 {
                continue;
            }
            let Some(sfx) = usize::try_from(ent.sound)
                .ok()
                .and_then(|s| self.precache.get(s).copied().flatten())
                .filter(|&id| self.sound(id).is_some())
            else {
                continue;
            };
            let (mut left, mut right) =
                spatialize_origin(ent.origin, listener, 255.0, LOOP_ATTENUATE);
            for (j, other) in entities.iter().enumerate().skip(i + 1) {
                if other.sound == ent.sound {
                    done[j] = true;
                    let (l, r) = spatialize_origin(other.origin, listener, 255.0, LOOP_ATTENUATE);
                    left += l;
                    right += r;
                }
            }
            if left == 0 && right == 0 {
                continue;
            }
            let Some(ch) = self.pick_channel(0, 0, now_ms, &mut mix.commands) else {
                return;
            };
            let (left, right) = (left.min(255), right.min(255));
            let length_ms = self.sound(sfx).map_or(0.0, |s| s.duration_ms());
            let pos = if length_ms > 0.0 {
                now_ms % length_ms
            } else {
                0.0
            };
            self.channels[ch] = Channel {
                sfx: Some(sfx),
                leftvol: left,
                rightvol: right,
                autosound: true,
                end_ms: now_ms + length_ms - pos,
                ..Channel::default()
            };
            mix.loops.push(LoopMix {
                sfx,
                left: gain(left),
                right: gain(right),
            });
        }
    }
}

#[cfg(test)]
mod tests;
