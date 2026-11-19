//! The server's save files (C `sv_save.c`) for the in-process game.
//!
//! Each slot holds what C writes under `save/<slot>/`: `server.ssv` (the
//! level to load and the latched cvars), `game.ssv` (the game's
//! cross-level state), and per visited level `<map>.sv2` (configstrings
//! and area portals) and `<map>.sav` (the game's level). `current` is the
//! live slot a unit's levels are kept in; `save0` is the autosave written
//! on entering a level.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use q2_common::error::{Q2Error, Q2Result};
use q2_game::traits::GameImport;
use q2_shared::constants::STAT_HEALTH;

use super::{new_game, LocalGame, LOCAL_PLAYER, SERVER_PRINT_TARGET};
use crate::init::MapRequest;
use crate::state::ServerState;

/// The cvars C saves as CVAR_LATCH ones: they change only with a new game.
const LATCHED: &[&str] = &["skill", "coop", "deathmatch"];

/// Frames a revisited level runs before the player arrives (C
/// SV_CheckForSavegame: "run it for ten seconds").
const REVISIT_FRAMES: usize = 100;

/// Every slot's files by `<slot>/<file>`. The platform persists them.
#[derive(Debug, Default, Clone)]
pub struct SaveFiles {
    files: BTreeMap<String, Vec<u8>>,
    /// Paths written or removed since `take_changes`, for the platform's
    /// copy to follow.
    changed: std::collections::BTreeSet<String>,
}

impl SaveFiles {
    /// `<slot>/<name>`, if written.
    pub fn file(&self, slot: &str, name: &str) -> Option<&[u8]> {
        self.files.get(&format!("{slot}/{name}")).map(Vec::as_slice)
    }

    /// Every file as (`<slot>/<file>`, bytes), for the platform to store.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.files
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
    }

    /// The slot of a `<slot>/<file>` path: the text before its first
    /// slash, or "" for a path with none. Every split of a path goes
    /// through `split`, so the server and the platform agree on it.
    pub fn slot_of(path: &str) -> &str {
        split(path).map_or("", |(slot, _)| slot)
    }

    /// Put back a file the platform stored, by its `iter` path. It is
    /// not a change: the platform has it already. A path no save could
    /// have is ignored.
    pub fn insert(&mut self, path: &str, bytes: Vec<u8>) {
        if !stored_path(path) {
            tracing::warn!("save file {path:?} ignored: not <slot>/<file>");
            return;
        }
        self.changed.remove(path);
        self.files.insert(path.to_string(), bytes);
    }

    /// Drop a file the platform stored, so its copy is removed too. A
    /// path no save could have is ignored, as `insert` ignores it.
    pub fn discard(&mut self, path: &str) {
        if !stored_path(path) {
            tracing::warn!("save file {path:?} not discarded: not <slot>/<file>");
            return;
        }
        self.files.remove(path);
        self.changed.insert(path.to_string());
    }

    /// The level string `slot` loads (C `svs.mapcmd`), if it holds a
    /// readable `server.ssv`.
    pub fn level(&self, slot: &str) -> Option<String> {
        decode::<ServerFile>(self.file(slot, "server.ssv")?)
            .ok()
            .map(|server| server.mapcmd)
    }

    /// Each path written or removed since the last call, with its bytes
    /// or `None` for removed, in path order.
    pub fn take_changes(&mut self) -> Vec<(String, Option<Vec<u8>>)> {
        std::mem::take(&mut self.changed)
            .into_iter()
            .map(|path| {
                let bytes = self.files.get(&path).cloned();
                (path, bytes)
            })
            .collect()
    }

    fn put(&mut self, slot: &str, name: &str, bytes: Vec<u8>) {
        let path = format!("{slot}/{name}");
        self.changed.insert(path.clone());
        self.files.insert(path, bytes);
    }

    /// C `SV_WipeSavegame`.
    pub(super) fn wipe(&mut self, slot: &str) {
        let changed = &mut self.changed;
        self.files.retain(|path, _| {
            let keep = Self::slot_of(path) != slot;
            if !keep {
                changed.insert(path.clone());
            }
            keep
        });
    }

    /// C `SV_CopySaveGame`: `dst` becomes a copy of `src`.
    pub(super) fn copy(&mut self, src: &str, dst: &str) {
        self.wipe(dst);
        let copies: Vec<_> = self
            .files
            .iter()
            .filter_map(|(path, bytes)| match split(path)? {
                (slot, name) if slot == src => Some((format!("{dst}/{name}"), bytes.clone())),
                _ => None,
            })
            .collect();
        for (path, bytes) in copies {
            self.changed.insert(path.clone());
            self.files.insert(path, bytes);
        }
    }

    /// Slots holding a `server.ssv`, the newest first.
    pub fn slots(&self) -> Vec<String> {
        let mut slots: Vec<(u64, String)> = self
            .files
            .iter()
            .filter_map(|(path, bytes)| {
                let (slot, name) = split(path)?;
                if slot == "current" || name != "server.ssv" {
                    return None;
                }
                Some((decode::<ServerFile>(bytes).ok()?.sequence, slot.to_string()))
            })
            .collect();
        slots.sort_unstable_by(|a, b| b.cmp(a));
        slots.into_iter().map(|(_, slot)| slot).collect()
    }
}

/// C `server.ssv`.
#[derive(Serialize, Deserialize)]
struct ServerFile {
    /// What the load menu shows: the level name, or "ENTERING <level>".
    comment: String,
    /// The level string to load (C `svs.mapcmd`).
    mapcmd: String,
    latched: Vec<(String, String)>,
    /// Orders the saves, newest highest, for the death menu stand-in.
    sequence: u64,
}

/// C `<map>.sv2`.
#[derive(Serialize, Deserialize)]
struct LevelFile {
    configstrings: Vec<String>,
    portals: Vec<bool>,
}

impl LevelFile {
    /// Sizes a decoded file could have but a map cannot: reading one would
    /// fail after the load gave up the running game.
    fn check(&self) -> Q2Result<()> {
        if self.configstrings.len() > q2_shared::constants::MAX_CONFIGSTRINGS
            || self.portals.len() != q2_common::collision::MAX_MAP_AREAPORTALS
        {
            return Err(Q2Error::Drop("level file sizes are damaged".into()));
        }
        Ok(())
    }
}

const MAGIC: &[u8; 8] = b"Q2RS-SRV";
const VERSION: u8 = 1;

fn encode<T: Serialize>(body: &T) -> Q2Result<Vec<u8>> {
    let mut out = MAGIC.to_vec();
    out.push(VERSION);
    postcard::to_extend(body, out).map_err(|e| Q2Error::Drop(format!("save failed: {e}")))
}

fn decode<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Q2Result<T> {
    match bytes.strip_prefix(MAGIC.as_slice()) {
        Some([VERSION, rest @ ..]) => postcard::from_bytes(rest)
            .map_err(|e| Q2Error::Drop(format!("savegame is damaged: {e}"))),
        _ => Err(Q2Error::Drop(
            "savegame from an incompatible version".into(),
        )),
    }
}

/// `<slot>/<file>` split at the first slash.
fn split(path: &str) -> Option<(&str, &str)> {
    path.split_once('/')
}

/// Whether `path` is one a save writes: a good slot and a file name
/// with no slash, so `split` reads back the slot it was written under.
fn stored_path(path: &str) -> bool {
    split(path)
        .is_some_and(|(slot, name)| !bad_slot(slot) && !name.is_empty() && !name.contains('/'))
}

/// C refuses these as save names ("Bad savedir."), so a name cannot
/// reach outside its slot.
fn bad_slot(slot: &str) -> bool {
    slot.is_empty() || slot.contains("..") || slot.contains('/') || slot.contains('\\')
}

impl LocalGame {
    /// The save slots, for the platform to persist and restore.
    pub fn saves(&self) -> &SaveFiles {
        &self.saves
    }

    pub fn saves_mut(&mut self) -> &mut SaveFiles {
        &mut self.saves
    }

    /// C `SV_WriteLevelFile`: the running level into `current`. Without
    /// `client_bodies` the player is saved as gone (a level being left).
    pub(super) fn write_level_file(&mut self, client_bodies: bool) -> Q2Result<()> {
        self.sv.sync_configstrings(&self.gi);
        let level = LevelFile {
            configstrings: self.sv.configstrings.clone(),
            portals: self.gi.portal_states(),
        };
        let name = self.sv.name.clone();
        self.saves
            .put("current", &format!("{name}.sv2"), encode(&level)?);
        let game_level = self.game.write_level(client_bodies)?;
        self.saves
            .put("current", &format!("{name}.sav"), game_level);
        Ok(())
    }

    /// C `SV_ReadLevelFile`.
    fn read_level_file(&mut self) -> Q2Result<()> {
        let name = self.sv.name.clone();
        let missing = || Q2Error::Drop(format!("Failed to open save/current/{name}"));
        let level: LevelFile = decode(
            self.saves
                .file("current", &format!("{name}.sv2"))
                .ok_or_else(missing)?,
        )?;
        let game_level = self
            .saves
            .file("current", &format!("{name}.sav"))
            .ok_or_else(missing)?
            .to_vec();
        for (index, value) in level.configstrings.iter().enumerate() {
            if self.sv.configstrings.get(index) != Some(value) {
                self.gi.configstring(index as i32, value);
            }
        }
        self.sv.sync_configstrings(&self.gi);
        self.gi.set_portal_states(&level.portals)?;
        self.game.read_level(&game_level)?;
        self.sv.drop_on_game_error(&self.gi)
    }

    /// C `SV_WriteServerFile` and the game's `WriteGame`, into `current`.
    pub(super) fn write_server_file(&mut self, autosave: bool) -> Q2Result<()> {
        let level_name = self.sv.configstrings[q2_shared::constants::CS_NAME].clone();
        let sequence = self
            .saves
            .iter()
            .filter(|(path, _)| split(path).is_some_and(|(_, name)| name == "server.ssv"))
            .filter_map(|(_, bytes)| decode::<ServerFile>(bytes).ok())
            .map(|file| file.sequence)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        let server = ServerFile {
            comment: match autosave {
                true => format!("ENTERING {level_name}"),
                false => level_name,
            },
            mapcmd: self.svs.mapcmd.clone(),
            latched: LATCHED
                .iter()
                .map(|name| (name.to_string(), self.gi.cvar(name, "")))
                .filter(|(_, value)| !value.is_empty())
                .collect(),
            sequence,
        };
        self.saves.put("current", "server.ssv", encode(&server)?);
        let game = self.game.write_game(autosave)?;
        self.saves.put("current", "game.ssv", game);
        Ok(())
    }

    /// C `SV_ReadServerFile`: the latched cvars, a fresh game (C
    /// `SV_InitGame`, which drops the client to reconnect), and its
    /// `ReadGame`. Returns the level to load.
    fn read_server_file(&mut self) -> Q2Result<String> {
        let missing = || Q2Error::Drop("Couldn't read save/current/server.ssv".into());
        let server: ServerFile = decode(
            self.saves
                .file("current", "server.ssv")
                .ok_or_else(missing)?,
        )?;
        let game = self
            .saves
            .file("current", "game.ssv")
            .ok_or_else(missing)?
            .to_vec();
        for (name, value) in &server.latched {
            self.gi.set_cvar(name, value);
        }
        self.game = new_game(&self.gi);
        self.svs.init_game(1);
        self.gi
            .set_client_states(self.svs.clients.iter().map(|client| client.state));
        self.svs.mapcmd = server.mapcmd.clone();
        self.game.read_game(&game)?;
        self.sv.drop_on_game_error(&self.gi)?;
        Ok(server.mapcmd)
    }

    /// C `SV_CheckForSavegame`, after a map spawned: a level the unit
    /// visited before replaces the fresh one. Coming back to it (not a
    /// manual load) runs it ten seconds first, as if time passed there.
    pub(super) fn check_for_savegame(&mut self, loading: Option<bool>) -> Q2Result<()> {
        let deathmatch = self.gi.cvar("deathmatch", "0") != "0";
        if deathmatch
            || self
                .saves
                .file("current", &format!("{}.sav", self.sv.name))
                .is_none()
        {
            return Ok(());
        }
        self.gi.clear_world();
        self.read_level_file()?;
        let revisit = !matches!(loading, Some(false));
        if revisit {
            let state = std::mem::replace(&mut self.sv.state, ServerState::Loading);
            for _ in 0..REVISIT_FRAMES {
                self.game.run_frame();
                self.sv.drop_on_game_error(&self.gi)?;
            }
            self.sv.state = state;
            // What those frames sent has no client to go to.
            self.gi.take_messages();
            self.gi.take_sounds();
        }
        self.sv.sync_configstrings(&self.gi);
        Ok(())
    }

    /// C `SV_Savegame_f`.
    fn save_game(&mut self, slot: &str) -> Q2Result<()> {
        let dead = self
            .game
            .client_player_state(LOCAL_PLAYER)
            .is_some_and(|ps| ps.stats[STAT_HEALTH] <= 0);
        let refusal = if self.sv.state != ServerState::Game {
            Some("You must be in a game to save.")
        } else if self.gi.cvar("deathmatch", "0") != "0" {
            Some("Can't savegame in a deathmatch")
        } else if slot == "current" {
            Some("Can't save to 'current'")
        } else if bad_slot(slot) {
            Some("Bad savedir.")
        } else if dead {
            Some("Can't savegame while dead!")
        } else {
            None
        };
        if let Some(refusal) = refusal {
            tracing::info!(target: SERVER_PRINT_TARGET, "{refusal}");
            return Ok(());
        }
        tracing::info!(target: SERVER_PRINT_TARGET, "Saving game...");
        // The player stays in the level: loading it puts them back there.
        self.write_level_file(true)?;
        self.write_server_file(false)?;
        self.saves.copy("current", slot);
        tracing::info!(target: SERVER_PRINT_TARGET, "Done.");
        Ok(())
    }

    /// C `SV_Loadgame_f`, up to the map change: the platform reads the
    /// level's map for `take_map_change` and `change_map` finishes it.
    fn load_game(&mut self, slot: &str) -> Q2Result<()> {
        if bad_slot(slot) {
            tracing::info!(target: SERVER_PRINT_TARGET, "Bad savedir.");
            return Ok(());
        }
        if self.saves.file(slot, "server.ssv").is_none() {
            tracing::info!(target: SERVER_PRINT_TARGET, "No such savegame: {slot}");
            return Ok(());
        }
        // C reads the files only after giving up the running game, so a bad
        // one ends it; here a bad slot is refused and play goes on.
        if let Err(e) = self.check_slot(slot) {
            tracing::info!(target: SERVER_PRINT_TARGET, "Couldn't load {slot}: {e}");
            return Ok(());
        }
        tracing::info!(target: SERVER_PRINT_TARGET, "Loading game...");
        if slot != "current" {
            self.saves.copy(slot, "current");
        }
        let mapcmd = self.read_server_file()?;
        // C sv.state = ss_dead: the level being left is not saved.
        self.sv.state = ServerState::Dead;
        self.loading = Some(slot == "save0" || slot == "current");
        *self.next_map.borrow_mut() = Some(mapcmd);
        Ok(())
    }

    /// Every file of `slot` decodes in this build, none the load reads is
    /// missing (`game.ssv`, and each level's `.sv2` beside its `.sav`),
    /// and the game data has the map it loads. `load` runs it before it
    /// gives up the running game; the platform runs it on a slot it
    /// restored, to drop one no load could read.
    pub fn check_slot(&self, slot: &str) -> Q2Result<()> {
        let missing = |name: &str| Q2Error::Drop(format!("{name} is missing"));
        if let Some(map) = self
            .saves
            .level(slot)
            .and_then(|level| MapRequest::destination(&level))
            .filter(|map| !(self.has_map)(map))
        {
            return Err(Q2Error::Drop(format!(
                "maps/{map}.bsp is not in the game data"
            )));
        }
        self.saves
            .file(slot, "game.ssv")
            .ok_or_else(|| missing("game.ssv"))?;
        for (path, bytes) in self.saves.iter() {
            let Some((_, name)) = split(path).filter(|(owner, _)| *owner == slot) else {
                continue;
            };
            match name.rsplit_once('.').map(|(_, ext)| ext) {
                _ if name == "server.ssv" => decode::<ServerFile>(bytes).map(drop),
                _ if name == "game.ssv" => self.game.check_game(bytes),
                Some("sv2") => decode::<LevelFile>(bytes).and_then(|level| level.check()),
                Some("sav") => {
                    let sv2 = format!("{}.sv2", &name[..name.len() - 4]);
                    match self.saves.file(slot, &sv2) {
                        Some(_) => self.game.check_level(bytes),
                        None => Err(missing(&sv2)),
                    }
                }
                _ => Ok(()),
            }
            .map_err(|e| Q2Error::Drop(format!("{name}: {e}")))?;
        }
        Ok(())
    }

    /// The console commands queued since the last call, run in order.
    pub(super) fn run_server_commands(&mut self) -> Q2Result<()> {
        let commands = std::mem::take(&mut *self.server_commands.borrow_mut());
        for args in commands {
            match args.as_slice() {
                [cmd, slot] if cmd == "save" => self.save_game(slot)?,
                [cmd, slot] if cmd == "load" => self.load_game(slot)?,
                // C pushes the load menu; with no menu, load the newest save.
                [cmd] if cmd == "menu_loadgame" => match self
                    .saves
                    .slots()
                    .into_iter()
                    .find(|slot| self.check_slot(slot).is_ok())
                {
                    Some(slot) => self.load_game(&slot.clone())?,
                    None => tracing::info!(target: SERVER_PRINT_TARGET, "No savegame to load"),
                },
                // C prints the usage under the long names.
                [cmd, ..] if cmd == "save" => {
                    tracing::info!(target: SERVER_PRINT_TARGET, "USAGE: savegame <directory>")
                }
                [cmd, ..] if cmd == "load" => {
                    tracing::info!(target: SERVER_PRINT_TARGET, "USAGE: loadgame <directory>")
                }
                [cmd, ..] => tracing::info!(target: SERVER_PRINT_TARGET, "USAGE: {cmd}"),
                [] => {}
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_name_every_path_written_or_removed_since_the_last_take() {
        let mut saves = SaveFiles::default();
        saves.insert("quick/server.ssv", vec![1]);
        assert!(
            saves.take_changes().is_empty(),
            "a restored file is not a change"
        );

        saves.put("current", "base1.sav", vec![2]);
        saves.copy("current", "save0");
        saves.copy("quick", "current");
        let changes = saves.take_changes();
        assert_eq!(
            changes,
            vec![
                ("current/base1.sav".to_string(), None),
                ("current/server.ssv".to_string(), Some(vec![1])),
                ("save0/base1.sav".to_string(), Some(vec![2])),
            ]
        );
        assert!(saves.take_changes().is_empty());
    }

    #[test]
    fn restoring_a_file_written_this_visit_takes_back_its_change() {
        let mut saves = SaveFiles::default();
        saves.put("save0", "server.ssv", vec![1]);
        saves.insert("save0/server.ssv", vec![2]);
        assert!(saves.take_changes().is_empty(), "the platform has it");
    }

    #[test]
    fn discarding_a_stored_file_removes_it_from_the_platform() {
        let mut saves = SaveFiles::default();
        saves.discard("current/old.sav");
        assert_eq!(
            saves.take_changes(),
            vec![("current/old.sav".to_string(), None)]
        );
    }

    fn server_file(sequence: u64, mapcmd: &str) -> Vec<u8> {
        encode(&ServerFile {
            comment: String::new(),
            mapcmd: mapcmd.into(),
            latched: Vec::new(),
            sequence,
        })
        .unwrap()
    }

    #[test]
    fn a_save_after_one_numbered_at_the_limit_stays_at_the_limit() {
        let mut game =
            LocalGame::start("floor_wall", &crate::test_bsp::floor_wall_bsp()).expect("map loads");
        game.saves
            .insert("odd/server.ssv", server_file(u64::MAX, "floor_wall"));
        game.write_server_file(false).unwrap();
        let current: ServerFile =
            decode(game.saves.file("current", "server.ssv").unwrap()).unwrap();
        assert_eq!(current.sequence, u64::MAX);
    }

    #[test]
    fn the_server_file_layout_is_pinned_so_a_change_bumps_version() {
        // postcard writes fields by position: a reordered or retyped
        // field misreads older saves. Every field holds a distinct value,
        // so any such change moves the bytes. If this fails, bump VERSION
        // and update the pin.
        let server = encode(&ServerFile {
            comment: "ENTERING Outer Base".into(),
            mapcmd: "*base1$start".into(),
            latched: vec![("skill".into(), "2".into())],
            sequence: 7,
        })
        .unwrap();
        let level = encode(&LevelFile {
            configstrings: vec!["a".into(), String::new(), "b".into()],
            portals: vec![true, false],
        })
        .unwrap();
        let fingerprint = |bytes: &[u8]| {
            bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, &b| {
                (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
            })
        };
        assert_eq!(
            (VERSION, fingerprint(&server), fingerprint(&level)),
            (1, 1_676_326_312_022_693_364, 6_933_407_155_205_816_557),
            "a server save type changed: bump VERSION"
        );
    }

    #[test]
    fn a_level_file_sized_for_no_map_is_refused() {
        let good = LevelFile {
            configstrings: vec![String::new(); q2_shared::constants::MAX_CONFIGSTRINGS],
            portals: vec![false; q2_common::collision::MAX_MAP_AREAPORTALS],
        };
        assert!(good.check().is_ok());
        let short_portals = LevelFile {
            portals: vec![false; 3],
            ..good
        };
        assert!(short_portals.check().is_err());
        let long_configstrings = LevelFile {
            configstrings: vec![String::new(); q2_shared::constants::MAX_CONFIGSTRINGS + 1],
            portals: vec![false; q2_common::collision::MAX_MAP_AREAPORTALS],
        };
        assert!(long_configstrings.check().is_err());
    }

    #[test]
    fn a_path_a_save_could_not_have_is_not_stored() {
        let mut saves = SaveFiles::default();
        saves.insert("good/server.ssv", server_file(1, "base1"));
        for bad in [
            "a/b/server.ssv",
            "../server.ssv",
            "/server.ssv",
            "server.ssv",
            "x/",
        ] {
            saves.insert(bad, server_file(2, "base1"));
            saves.discard(bad);
        }
        assert_eq!(saves.slots(), ["good"]);
        assert_eq!(saves.iter().count(), 1);
        assert!(saves.take_changes().is_empty());
    }

    #[test]
    fn slot_of_names_the_slot_before_the_slash() {
        assert_eq!(SaveFiles::slot_of("quick/base1.sav"), "quick");
        assert_eq!(SaveFiles::slot_of("server.ssv"), "");
    }

    #[test]
    fn a_slot_names_the_level_it_loads() {
        let mut saves = SaveFiles::default();
        saves.insert("quick/server.ssv", server_file(1, "*base2$start"));
        assert_eq!(saves.level("quick").as_deref(), Some("*base2$start"));
        assert_eq!(saves.level("none"), None);
    }
}
