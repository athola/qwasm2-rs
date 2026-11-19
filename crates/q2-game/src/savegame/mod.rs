//! Save and load of the game and level state (C `savegame/savegame.c`).
//!
//! A game file holds what survives a level change (`GameLocals` and every
//! client record); a level file holds one level's `LevelLocals` and
//! entities. The server stores them as C's `game.ssv` and `<map>.sav`.
//! The encoding is the port's own (postcard): C writes its structs as they
//! lie in memory, with function and move pointers swapped for names, so its
//! files are tied to C's struct layout, OS and architecture, and there is
//! nothing to be compatible with.

pub mod fields;
pub mod registry;

use serde::{Deserialize, Serialize};

use q2_common::error::{Q2Error, Q2Result};
use q2_shared::constants::MAX_CLIENTS;

use crate::ai::EnemyCheck;
use crate::constants::{MeansOfDeath, BODY_QUEUE_SIZE};
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
pub(crate) struct GameSave {
    game: GameLocals,
    clients: Vec<ClientData>,
}

/// One level. Besides C's `level` and edicts this keeps the game's file
/// statics, so a loaded level runs on exactly as it would have.
#[derive(Serialize, Deserialize)]
pub(crate) struct LevelSave {
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

impl GameSave {
    /// Values a decoded save could hold but play would index out of
    /// range: a client count the server cannot hold or that disagrees with
    /// the clients, and item indices past the item list.
    fn check(&self, items: usize) -> Q2Result<()> {
        if !(0..=MAX_CLIENTS as i32).contains(&self.game.maxclients()) {
            return Err(Q2Error::Drop("savegame maxclients is out of range".into()));
        }
        if self.clients.len() != self.game.maxclients() as usize {
            return Err(Q2Error::Drop("savegame client count is damaged".into()));
        }
        let item = |index: Option<usize>| index.is_none_or(|i| i < items);
        for client in &self.clients {
            for (pers, prefix) in [
                (&client.pers, ""),
                (&client.resp.coop_respawn, "coop_respawn."),
            ] {
                let bad = if !(-1..items as i32).contains(&pers.selected_item) {
                    Some("selected_item")
                } else if !item(pers.weapon) || !item(pers.last_weapon) {
                    Some("weapon")
                } else if pers.inventory[items.min(pers.inventory.len())..]
                    .iter()
                    .any(|&n| n != 0)
                {
                    Some("inventory")
                } else {
                    None
                };
                if let Some(field) = bad {
                    return Err(Q2Error::Drop(format!(
                        "savegame {prefix}{field} is out of range"
                    )));
                }
            }
            if !item(client.newweapon) {
                return Err(Q2Error::Drop("savegame newweapon is out of range".into()));
            }
            if client.ammo_index >= client.pers.inventory.len() {
                return Err(Q2Error::Drop("savegame ammo_index is out of range".into()));
            }
        }
        Ok(())
    }
}

impl LevelSave {
    /// Values play would index out of range: the entity numbering, the
    /// trail, the pain cycle (a three-entry table) and the body queue slot
    /// (an entity number past the clients).
    fn check(&self) -> Q2Result<()> {
        let range = |field: &str, value: i32, end: usize| match (0..end as i32).contains(&value) {
            true => Ok(()),
            false => Err(format!("{field} is out of range")),
        };
        self.entities
            .check()
            .and_then(|()| self.trail.check())
            .and_then(|()| range("pain_anim_cycle", self.pain_anim_cycle, 3))
            .and_then(|()| range("body_que", self.level.body_que, BODY_QUEUE_SIZE))
            .map_err(|e| Q2Error::Drop(format!("savegame {e}")))
    }
}

/// A game file, refused if its values would break play.
pub(crate) fn decode_game(bytes: &[u8]) -> Q2Result<GameSave> {
    let save: GameSave = decode(KIND_GAME, bytes)?;
    // C's item list is static: the same in every game, initialized or not.
    save.check(crate::items::build_item_list().len())?;
    Ok(save)
}

/// A level file, refused if its values would break play.
pub(crate) fn decode_level(bytes: &[u8]) -> Q2Result<LevelSave> {
    let save: LevelSave = decode(KIND_LEVEL, bytes)?;
    save.check()?;
    Ok(save)
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
        let GameSave { game, clients } = decode_game(bytes)?;
        self.set_max_clients(game.maxclients().max(0) as usize);
        self.game = game;
        self.clients = clients;
        Ok(())
    }

    /// C `WriteLevel`: the level locals and its entities, client bodies
    /// included (`GameExport::write_level` can leave them out).
    pub fn encode_level(&self) -> Q2Result<Vec<u8>> {
        self.check_names()?;
        encode(KIND_LEVEL, &self.level_save())
    }

    fn level_save(&self) -> LevelSave {
        LevelSave {
            level: self.level.clone(),
            entities: self.entities.clone(),
            trail: self.trail.clone(),
            enemy_check: self.enemy_check,
            means_of_death: self.means_of_death,
            friendly_fire: self.friendly_fire,
            quad_drop_timeout_hack: self.quad_drop_timeout_hack,
            pain_anim_cycle: self.pain_anim_cycle,
            is_quad: self.is_quad,
        }
    }

    /// C `ReadLevel`. The server has cleared its world links and connected
    /// no clients yet. Like C, a freed entity comes back zeroed (C writes
    /// only the edicts in use), every entity in use is linked again, the
    /// clients are marked unconnected, and crosslevel targets start their
    /// countdown.
    pub fn read_level(&mut self, bytes: &[u8]) -> Q2Result<()> {
        let save = decode_level(bytes)?;
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
        let time = self.level.time;
        for (_, key) in numbered {
            let ent = self.entities.get_mut(key).expect("key just listed");
            if ent.in_use {
                if ent.game.classname == "target_crosslevel_target" {
                    ent.game.nextthink = time + ent.game.delay;
                }
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
            let mut fields = vec![
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
            ];
            if let Some(info) = &ent.monsterinfo {
                fields.extend([
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
                ]);
            }
            let missing: Vec<_> = fields
                .into_iter()
                .filter_map(|(field, ok)| (!ok).then_some(field))
                .collect();
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
