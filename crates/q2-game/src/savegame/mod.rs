//! Save and load of the game and level state (C `savegame/savegame.c`).
//!
//! A game file holds what survives a level change (`GameLocals` and every
//! client record); a level file holds one level's `LevelLocals` and
//! entities. The server stores them as C's `game.ssv` and `<map>.sav`.
//! The encoding is the port's own (postcard): C's files are raw struct
//! dumps tied to one build, so there is nothing to be compatible with.

pub mod fields;
pub mod registry;

use serde::{Deserialize, Serialize};

use q2_common::error::{Q2Error, Q2Result};

use crate::ai::EnemyCheck;
use crate::constants::MeansOfDeath;
use crate::entity::{ClientData, Entity, EntityStorage};
use crate::player::trail::PlayerTrail;
use crate::world::{GameLocals, GameWorld, LevelLocals};

/// Leads every save file, so a stray file is refused before decoding.
const MAGIC: &[u8; 8] = b"Q2RS-SAV";
/// Bumped whenever a saved type changes shape; older files are refused
/// (C `SAVEGAMEVER`).
const VERSION: u8 = 1;
const KIND_GAME: u8 = b'G';
const KIND_LEVEL: u8 = b'L';

#[derive(Serialize, Deserialize)]
struct GameSave {
    game: GameLocals,
    clients: Vec<ClientData>,
}

/// One level. Besides C's `level` and edicts this keeps the game's file
/// statics, so a loaded level runs on exactly as it would have.
#[derive(Serialize, Deserialize)]
struct LevelSave {
    level: LevelLocals,
    entities: EntityStorage,
    trail: PlayerTrail,
    enemy_check: EnemyCheck,
    means_of_death: MeansOfDeath,
    friendly_fire: bool,
    quad_drop_timeout_hack: i32,
    pain_anim_cycle: i32,
    is_quad: bool,
}

fn encode<T: Serialize>(kind: u8, body: &T) -> Q2Result<Vec<u8>> {
    let mut out = MAGIC.to_vec();
    out.extend([VERSION, kind]);
    postcard::to_extend(body, out).map_err(|e| Q2Error::Drop(format!("save failed: {e}")))
}

fn decode<T: for<'de> Deserialize<'de>>(kind: u8, bytes: &[u8]) -> Q2Result<T> {
    let body = bytes
        .strip_prefix(MAGIC.as_slice())
        .ok_or_else(|| Q2Error::Drop("not a savegame".into()))?;
    match body {
        [VERSION, k, rest @ ..] if *k == kind => match postcard::take_from_bytes(rest) {
            Ok((value, [])) => Ok(value),
            Ok(_) => Err(Q2Error::Drop("savegame has trailing data".into())),
            Err(e) => Err(Q2Error::Drop(format!("savegame is damaged: {e}"))),
        },
        [VERSION, ..] => Err(Q2Error::Drop("savegame is of another kind".into())),
        _ => Err(Q2Error::Drop(
            "savegame from an incompatible version".into(),
        )),
    }
}

/// Whether `bytes` is a game file this build can read, without reading it
/// into a game: a load checks its files before it gives up the running one.
pub fn check_game_file(bytes: &[u8]) -> Q2Result<()> {
    decode::<GameSave>(KIND_GAME, bytes).map(drop)
}

/// `check_game_file` for a level file.
pub fn check_level_file(bytes: &[u8]) -> Q2Result<()> {
    decode::<LevelSave>(KIND_LEVEL, bytes).map(drop)
}

impl GameWorld {
    /// C `WriteGame`: the cross-level state and every client. A manual save
    /// first copies each player's health and flags into `pers`
    /// (`SaveClientData`); an autosave, written on entering a level, keeps
    /// what the level change stored. `game.autosaved` is set for the write
    /// only, so a load knows it came from an autosave.
    pub fn write_game(&mut self, autosave: bool) -> Q2Result<Vec<u8>> {
        if !autosave {
            self.save_client_data();
        }
        self.game.autosaved = autosave;
        let saved = encode(
            KIND_GAME,
            &GameSave {
                game: self.game.clone(),
                clients: self.clients.clone(),
            },
        );
        self.game.autosaved = false;
        saved
    }

    /// C `ReadGame`. The entities are the next level's business: the server
    /// spawns a map and then reads its level file.
    pub fn read_game(&mut self, bytes: &[u8]) -> Q2Result<()> {
        let GameSave { game, clients } = decode(KIND_GAME, bytes)?;
        if clients.len() != game.maxclients().max(0) as usize {
            return Err(Q2Error::Drop("savegame client count is damaged".into()));
        }
        self.set_max_clients(game.maxclients().max(0) as usize);
        self.game = game;
        self.clients = clients;
        Ok(())
    }

    /// C `WriteLevel`: the level locals and its entities.
    pub fn write_level(&self) -> Q2Result<Vec<u8>> {
        self.check_names()?;
        encode(
            KIND_LEVEL,
            &LevelSave {
                level: self.level.clone(),
                entities: self.entities.clone(),
                trail: self.trail.clone(),
                enemy_check: self.enemy_check,
                means_of_death: self.means_of_death,
                friendly_fire: self.friendly_fire,
                quad_drop_timeout_hack: self.quad_drop_timeout_hack,
                pain_anim_cycle: self.pain_anim_cycle,
                is_quad: self.is_quad,
            },
        )
    }

    /// C `ReadLevel`. The server has cleared its world links and connected
    /// no clients yet. Like C, a freed entity comes back zeroed (C writes
    /// only the edicts in use), every entity in use is linked again, the
    /// clients are marked unconnected, and crosslevel targets start their
    /// countdown.
    pub fn read_level(&mut self, bytes: &[u8]) -> Q2Result<()> {
        let save: LevelSave = decode(KIND_LEVEL, bytes)?;
        self.level = save.level;
        self.entities = save.entities;
        self.entities
            .set_max_clients(self.game.maxclients().max(0) as usize);
        self.entities.set_level_time(self.level.time);
        self.trail = save.trail;
        self.enemy_check = save.enemy_check;
        self.means_of_death = save.means_of_death;
        self.friendly_fire = save.friendly_fire;
        self.quad_drop_timeout_hack = save.quad_drop_timeout_hack;
        self.pain_anim_cycle = save.pain_anim_cycle;
        self.is_quad = save.is_quad;

        // C relinks in edict order; the server's area lists keep that order.
        let mut numbered: Vec<_> = self
            .entities
            .iter()
            .map(|(key, ent)| (ent.state.number, key))
            .collect();
        numbered.sort_unstable();
        let keys: Vec<_> = numbered.into_iter().map(|(_, key)| key).collect();
        for &key in &keys {
            let Some(ent) = self.entities.get_mut(key) else {
                continue;
            };
            if ent.in_use {
                ent.area_linked = false;
                self.link_entity(key);
            } else {
                let number = ent.state.number;
                *ent = Entity::default();
                ent.state.number = number;
            }
        }
        for slot in 0..self.clients.len() {
            if let Some(ent) = self
                .entities
                .key_for_number(slot + 1)
                .and_then(|key| self.entities.get_mut(key))
            {
                ent.client = Some(slot);
            }
            self.clients[slot].pers.connected = false;
        }
        let time = self.level.time;
        for &key in &keys {
            if let Some(ent) = self.entities.get_mut(key) {
                if ent.in_use && ent.game.classname == "target_crosslevel_target" {
                    ent.game.nextthink = time + ent.game.delay;
                }
            }
        }
        Ok(())
    }
}

impl GameWorld {
    /// Refuse a level holding a callback or monster move the registry
    /// lacks, naming it. The encoder would refuse it too, but postcard drops
    /// the message.
    fn check_names(&self) -> Q2Result<()> {
        use registry::{move_name, name_of, Callback};
        fn named<F: Callback>(f: Option<F>) -> bool {
            f.is_none_or(|f| name_of(f).is_some())
        }
        for (_, ent) in self.entities.iter() {
            let mut missing = Vec::new();
            for (field, ok) in [
                ("prethink", named(ent.prethink)),
                ("think", named(ent.think)),
                ("blocked", named(ent.blocked)),
                ("touch", named(ent.touch)),
                ("use", named(ent.use_fn)),
                ("pain", named(ent.pain)),
                ("die", named(ent.die)),
                (
                    "moveinfo.endfunc",
                    named(ent.moveinfo.as_ref().and_then(|m| m.endfunc)),
                ),
            ] {
                if !ok {
                    missing.push(field);
                }
            }
            if let Some(info) = &ent.monsterinfo {
                for (field, ok) in [
                    ("monsterinfo.stand", named(info.stand)),
                    ("monsterinfo.idle", named(info.idle)),
                    ("monsterinfo.search", named(info.search)),
                    ("monsterinfo.walk", named(info.walk)),
                    ("monsterinfo.run", named(info.run)),
                    ("monsterinfo.dodge", named(info.dodge)),
                    ("monsterinfo.attack", named(info.attack)),
                    ("monsterinfo.melee", named(info.melee)),
                    ("monsterinfo.sight", named(info.sight)),
                    ("monsterinfo.checkattack", named(info.checkattack)),
                    (
                        "monsterinfo.currentmove",
                        info.currentmove
                            .as_ref()
                            .is_none_or(|m| move_name(m).is_some()),
                    ),
                ] {
                    if !ok {
                        missing.push(field);
                    }
                }
            }
            if !missing.is_empty() {
                return Err(Q2Error::Drop(format!(
                    "save failed: entity {} ({}) has an unregistered callback in {}",
                    ent.state.number,
                    ent.game.classname,
                    missing.join(", ")
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
