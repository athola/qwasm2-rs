//! Entity spawning — entity-string parser and classname-to-function dispatch.
//!
//! Mirrors the C `g_spawn.c` spawn table and `ED_ParseEdict` parser.

use std::collections::HashMap;

use q2_common::error::{Q2Error, Q2Result};

use crate::entity::{EntityKey, EntityStorage};
use crate::monster::{
    berserker, boss2, boss3, brain, chick, flipper, floater, flyer, gladiator, gunner, hover,
    infantry, insane, medic, mutant, parasite, soldier, supertank, tank, turret,
};
use crate::world::GameWorld;
use crate::{func, misc, targets, triggers};

// ---------------------------------------------------------------------------
// Entity-string parser
// ---------------------------------------------------------------------------

/// Parse a Quake 2 entity string into a list of key/value dictionaries.
///
/// The format is:
/// ```text
/// {
/// "classname" "info_player_start"
/// "origin" "0 0 0"
/// }
/// ```
///
/// Tokens follow C `COM_Parse` (quoted strings or bare words, `//` comments).
/// Keys starting with `_` are editor-only comments and are discarded.
/// Malformed text is an error, as in C `SpawnEntities` / `ED_ParseEdict`,
/// so a corrupt entity lump never loads as a partial map.
pub fn parse_entity_string(entstring: &str) -> Q2Result<Vec<HashMap<String, String>>> {
    let mut result = Vec::new();
    let mut chars = entstring.chars().peekable();

    while let Some(token) = next_token(&mut chars) {
        if !token.starts_with('{') {
            return Err(Q2Error::Drop(format!(
                "ED_LoadFromFile: found {token} when expecting {{"
            )));
        }

        let mut entity: HashMap<String, String> = HashMap::new();
        loop {
            let key = next_token(&mut chars).ok_or_else(eof_without_brace)?;
            if key.starts_with('}') {
                break;
            }
            let value = next_token(&mut chars).ok_or_else(eof_without_brace)?;
            if value.starts_with('}') {
                return Err(Q2Error::Drop(
                    "ED_ParseEntity: closing brace without data".into(),
                ));
            }
            // Discard editor-only keys (leading underscore).
            if !key.starts_with('_') {
                entity.insert(key, value);
            }
        }

        if !entity.is_empty() {
            result.push(entity);
        }
    }

    Ok(result)
}

fn eof_without_brace() -> Q2Error {
    Q2Error::Drop("ED_ParseEntity: EOF without closing brace".into())
}

/// Next token per C `COM_Parse`; `None` at end of input.
fn next_token(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<String> {
    loop {
        skip_whitespace(chars);
        let mut lookahead = chars.clone();
        if lookahead.next() == Some('/') && lookahead.next() == Some('/') {
            chars.find(|&c| c == '\n');
            continue;
        }
        break;
    }
    match chars.peek()? {
        '"' => Some(parse_quoted_string(chars)),
        _ => {
            let mut word = String::new();
            while let Some(c) = chars.next_if(|c| !c.is_ascii_whitespace()) {
                word.push(c);
            }
            Some(word)
        }
    }
}

/// Advance past whitespace characters.
fn skip_whitespace(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    while chars.next_if(|c| c.is_ascii_whitespace()).is_some() {}
}

/// Parse a `"quoted string"`, consuming the surrounding double quotes.
/// Returns an empty string if the next character is not `"`.
fn parse_quoted_string(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    // Expect opening '"'
    match chars.peek() {
        Some('"') => {
            chars.next();
        }
        _ => return String::new(),
    }

    let mut s = String::new();
    loop {
        match chars.next() {
            Some('"') | None => break,
            Some('\\') => {
                // C ED_NewString: `\n` is a newline, any other escaped
                // character becomes a lone backslash; a final backslash stays.
                match chars.peek() {
                    Some('"') | None => s.push('\\'),
                    Some('n') => {
                        chars.next();
                        s.push('\n');
                    }
                    Some(_) => {
                        chars.next();
                        s.push('\\');
                    }
                }
            }
            Some(c) => s.push(c),
        }
    }
    s
}

// ---------------------------------------------------------------------------
// Spawn function type + table
// ---------------------------------------------------------------------------

/// Signature for a spawn function.
pub type SpawnFn = fn(&mut EntityStorage, EntityKey, &HashMap<String, String>);

/// The half of a C spawn function that needs `GameWorld` (configstrings,
/// sound indexes, random numbers, the item table), run by `call_spawn`
/// after the `SpawnFn`.
pub type FinishFn = fn(&mut GameWorld, EntityKey, &HashMap<String, String>);

/// A C `spawns[]` row: classname and spawn function, plus the model the C
/// spawn function sets (`gi.modelindex` or `self->model`) and its
/// `GameWorld` half, both applied by `call_spawn`.
pub type SpawnEntry = (
    &'static str,
    SpawnFn,
    Option<&'static str>,
    Option<FinishFn>,
);

/// Every classname with a spawn function, in C order. Items are not here:
/// C ED_CallSpawn looks them up in the item list first. C ref: `spawns[]`
/// (g_spawn.c).
#[rustfmt::skip]
pub static SPAWNS: &[SpawnEntry] = &[
    ("info_player_start", sp_info_player_start, None, None),
    ("info_player_deathmatch", sp_info_player_deathmatch, None, None),
    ("info_player_coop", sp_info_player_coop, None, Some(info_player_coop_finish)),
    ("info_player_intermission", sp_info_player_intermission, None, None),

    ("func_plat", func::sp_func_plat, None, Some(func::finish_plat)),
    ("func_button", func::sp_func_button, None, Some(func::finish_button)),
    ("func_door", func::sp_func_door, None, Some(func::finish_door)),
    ("func_door_secret", func::sp_func_door_secret, None, Some(func::finish_door_secret)),
    ("func_door_rotating", func::sp_func_door_rotating, None, Some(func::finish_door)),
    ("func_rotating", func::sp_func_rotating, None, None),
    ("func_train", func::sp_func_train, None, Some(func::finish_train)),
    ("func_water", func::sp_func_water, None, Some(func::finish_water)),
    ("func_conveyor", func::sp_func_conveyor, None, None),
    ("func_areaportal", func::sp_func_areaportal, None, None),
    ("func_clock", func::sp_func_nonsolid, None, None),
    ("func_wall", func::sp_func_generic, None, None),
    ("func_object", func::sp_func_generic, None, None),
    ("func_timer", func::sp_func_timer, None, Some(func::start_func_timer)),
    ("func_explosive", func::sp_func_explosive, None, None),
    ("func_killbox", func::sp_func_killbox, None, None),

    ("trigger_always", triggers::sp_trigger_always, None, Some(finish_trigger)),
    ("trigger_once", triggers::sp_trigger_once, None, Some(finish_trigger)),
    ("trigger_multiple", triggers::sp_trigger_multiple, None, Some(finish_trigger)),
    ("trigger_relay", triggers::sp_trigger_relay, None, None),
    ("trigger_push", triggers::sp_trigger_push, None, None),
    ("trigger_hurt", triggers::sp_trigger_hurt, None, None),
    ("trigger_key", triggers::sp_trigger_key, None, Some(finish_trigger)),
    ("trigger_counter", triggers::sp_trigger_counter, None, None),
    ("trigger_elevator", func::sp_trigger_elevator, None, None),
    ("trigger_gravity", triggers::sp_trigger_gravity, None, None),
    ("trigger_monsterjump", triggers::sp_trigger_monsterjump, None, None),

    ("target_temp_entity", targets::sp_target_generic, None, None),
    ("target_speaker", targets::sp_target_speaker, None, Some(GameWorld::finish_target_speaker)),
    ("target_explosion", targets::sp_target_explosion, None, None),
    ("target_changelevel", targets::sp_target_changelevel, None, None),
    ("target_secret", targets::sp_target_secret, None, None),
    ("target_goal", targets::sp_target_goal, None, None),
    ("target_splash", targets::sp_target_generic, None, None),
    ("target_spawner", targets::sp_target_generic, None, None),
    ("target_blaster", targets::sp_target_generic, None, None),
    ("target_crosslevel_trigger", targets::sp_target_generic, None, None),
    ("target_crosslevel_target", targets::sp_target_generic, None, None),
    ("target_laser", targets::sp_target_generic, None, None),
    ("target_help", targets::sp_target_generic, None, None),
    ("target_lightramp", targets::sp_target_generic, None, None),
    ("target_earthquake", targets::sp_target_generic, None, None),
    ("target_character", misc::sp_misc_generic, None, None),
    ("target_string", misc::sp_misc_generic, None, None),

    ("worldspawn", sp_worldspawn, None, Some(GameWorld::worldspawn_configstrings)),
    ("viewthing", misc::sp_misc_generic, None, None),

    ("light", sp_light, None, Some(GameWorld::spawn_light)),
    ("light_mine1", misc::sp_light_mine, Some("models/objects/minelite/light1/tris.md2"), None),
    ("light_mine2", misc::sp_light_mine, Some("models/objects/minelite/light2/tris.md2"), None),
    ("info_null", misc::sp_info_null, None, None),
    ("func_group", misc::sp_info_null, None, None),
    ("info_notnull", misc::sp_info_notnull, None, None),
    ("path_corner", misc::sp_path_corner, None, None),
    ("point_combat", misc::sp_point_combat, None, Some(misc::finish_point_combat)),

    ("misc_explobox", misc::sp_misc_explobox, Some("models/objects/barrels/tris.md2"), None),
    ("misc_banner", misc::sp_misc_generic, Some("models/objects/banner/tris.md2"), None),
    ("misc_satellite_dish", misc::sp_misc_generic, Some("models/objects/satellite/tris.md2"), None),
    ("misc_gib_arm", misc::sp_misc_gib, Some("models/objects/gibs/arm/tris.md2"), None),
    ("misc_gib_leg", misc::sp_misc_gib, Some("models/objects/gibs/leg/tris.md2"), None),
    ("misc_gib_head", misc::sp_misc_gib, Some("models/objects/gibs/head/tris.md2"), None),
    ("misc_insane", insane::sp_monster_insane, Some("models/monsters/insane/tris.md2"), Some(insane::insane_finish)),
    ("misc_deadsoldier", misc::sp_misc_generic, Some("models/deadbods/dude/tris.md2"), None),
    ("misc_viper", misc::sp_misc_generic, Some("models/ships/viper/tris.md2"), None),
    ("misc_viper_bomb", misc::sp_misc_generic, Some("models/objects/bomb/tris.md2"), None),
    ("misc_bigviper", misc::sp_misc_generic, Some("models/ships/bigviper/tris.md2"), None),
    ("misc_strogg_ship", misc::sp_misc_generic, Some("models/ships/strogg1/tris.md2"), None),
    ("misc_teleporter", misc::sp_misc_teleporter, Some("models/objects/dmspot/tris.md2"), None),
    ("misc_teleporter_dest", misc::sp_misc_teleporter_dest, Some("models/objects/dmspot/tris.md2"), None),
    ("misc_blackhole", misc::sp_misc_generic, Some("models/objects/black/tris.md2"), None),
    ("misc_eastertank", misc::sp_misc_generic, Some("models/monsters/tank/tris.md2"), None),
    ("misc_easterchick", misc::sp_misc_generic, Some("models/monsters/bitch/tris.md2"), None),
    ("misc_easterchick2", misc::sp_misc_generic, Some("models/monsters/bitch/tris.md2"), None),

    ("monster_berserk", berserker::sp_monster_berserker, Some("models/monsters/berserk/tris.md2"), Some(berserker::sp_monster_berserker_finish)),
    ("monster_gladiator", gladiator::sp_monster_gladiator, Some("models/monsters/gladiatr/tris.md2"), Some(gladiator::sp_monster_gladiator_finish)),
    ("monster_gunner", gunner::sp_monster_gunner, Some("models/monsters/gunner/tris.md2"), Some(gunner::finish_gunner)),
    ("monster_infantry", infantry::sp_monster_infantry, Some("models/monsters/infantry/tris.md2"), Some(infantry::finish_infantry)),
    ("monster_soldier_light", soldier::sp_monster_soldier_light, Some("models/monsters/soldier/tris.md2"), Some(soldier::finish_soldier_light)),
    ("monster_soldier", soldier::sp_monster_soldier, Some("models/monsters/soldier/tris.md2"), Some(soldier::finish_soldier)),
    ("monster_soldier_ss", soldier::sp_monster_soldier_ss, Some("models/monsters/soldier/tris.md2"), Some(soldier::finish_soldier_ss)),
    ("monster_tank", tank::sp_monster_tank, Some("models/monsters/tank/tris.md2"), Some(tank::tank_precache)),
    ("monster_tank_commander", tank::sp_monster_tank_commander, Some("models/monsters/tank/tris.md2"), Some(tank::tank_precache)),
    ("monster_medic", medic::sp_monster_medic, Some("models/monsters/medic/tris.md2"), Some(medic::medic_precache)),
    ("monster_flipper", flipper::sp_monster_flipper, Some("models/monsters/flipper/tris.md2"), Some(flipper::finish_flipper)),
    ("monster_chick", chick::sp_monster_chick, Some("models/monsters/bitch/tris.md2"), Some(chick::sp_monster_chick_finish)),
    ("monster_parasite", parasite::sp_monster_parasite, Some("models/monsters/parasite/tris.md2"), Some(parasite::finish_parasite)),
    ("monster_flyer", flyer::sp_monster_flyer, Some("models/monsters/flyer/tris.md2"), Some(flyer::finish_flyer)),
    ("monster_brain", brain::sp_monster_brain, Some("models/monsters/brain/tris.md2"), Some(brain::sp_monster_brain_finish)),
    ("monster_floater", floater::sp_monster_floater, Some("models/monsters/float/tris.md2"), Some(floater::finish_floater)),
    ("monster_hover", hover::sp_monster_hover, Some("models/monsters/hover/tris.md2"), Some(hover::finish_hover)),
    ("monster_mutant", mutant::sp_monster_mutant, Some("models/monsters/mutant/tris.md2"), Some(mutant::sp_monster_mutant_finish)),
    ("monster_supertank", supertank::sp_monster_supertank, Some("models/monsters/boss1/tris.md2"), Some(supertank::supertank_precache)),
    ("monster_boss2", boss2::sp_monster_boss2, Some("models/monsters/boss2/tris.md2"), Some(boss2::boss2_precache)),
    ("monster_boss3_stand", boss3::sp_monster_boss3_stand, Some("models/monsters/boss3/rider/tris.md2"), Some(boss3::boss3_stand_finish)),
    ("monster_makron", boss3::sp_monster_makron, Some("models/monsters/boss3/rider/tris.md2"), Some(boss3::makron_finish)),
    ("monster_jorg", boss3::sp_monster_jorg, Some("models/monsters/boss3/jorg/tris.md2"), Some(boss3::jorg_finish)),

    ("monster_commander_body", misc::sp_misc_generic, Some("models/monsters/commandr/tris.md2"), None),

    ("turret_breach", turret::sp_turret_breach, None, Some(turret::turret_breach_finish)),
    ("turret_base", turret::sp_turret_base, None, None),
    ("turret_driver", turret::sp_turret_driver, Some("models/monsters/infantry/tris.md2"), None),
];

/// The `SPAWNS` row for `classname`, searched in order as C ED_CallSpawn
/// walks `spawns[]`.
pub fn find_spawn(classname: &str) -> Option<&'static SpawnEntry> {
    SPAWNS.iter().find(|entry| entry.0 == classname)
}

/// `GameWorld`'s handle on `SPAWNS` for looking spawn functions up by name.
#[derive(Default)]
pub struct SpawnTable;

impl SpawnTable {
    pub fn new() -> Self {
        Self
    }

    /// Look up the spawn function for a given classname.
    pub fn get(&self, classname: &str) -> Option<&'static SpawnFn> {
        find_spawn(classname).map(|entry| &entry.1)
    }
}

/// The `GameWorld` half of C SP_trigger_multiple, _once, _always and _key;
/// `finish_trigger_spawn` picks by classname.
fn finish_trigger(world: &mut GameWorld, key: EntityKey, fields: &HashMap<String, String>) {
    let Some(classname) = world.entities.get(key).map(|e| e.game.classname.clone()) else {
        return;
    };
    world.finish_trigger_spawn(key, &classname, fields);
}

// ---------------------------------------------------------------------------
// Placeholder spawn functions
// ---------------------------------------------------------------------------

/// C `strtol(s, NULL, 10)`: optional sign and leading digits, 0 if none.
pub(crate) fn strtol(s: &str) -> i32 {
    let t = s.trim_start();
    let (sign, digits) = match t.strip_prefix('-') {
        Some(rest) => (-1i64, rest),
        None => (1, t.strip_prefix('+').unwrap_or(t)),
    };
    let n = digits
        .bytes()
        .take_while(u8::is_ascii_digit)
        .fold(0i64, |n, d| {
            (n * 10 + i64::from(d - b'0')).min(i64::from(u32::MAX))
        });
    (sign * n).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// C `sscanf("%f %f %f")`: missing components stay 0.
pub(crate) fn scan_vector(s: &str) -> q2_shared::types::Vec3f {
    let mut v = [0.0f32; 3];
    for (slot, part) in v.iter_mut().zip(s.split_whitespace()) {
        *slot = q2_common::cvar::parse_value(part);
    }
    q2_shared::types::Vec3f::from_array(v)
}

/// Set the generic entity fields from an entity-string block, as C
/// ED_ParseEdict does for every entity before its spawn function runs.
/// The block is a `HashMap`, so when it has both `angle` and `angles` which
/// one wins is unspecified (C: the later key in the file); keeping file
/// order would change `SpawnFn`'s signature across every spawn module.
/// Fields this port has no slot for yet (`sounds`, the spawn-temp keys)
/// are left to the spawn functions.
pub fn apply_spawn_fields(ent: &mut crate::entity::Entity, fields: &HashMap<String, String>) {
    use q2_common::cvar::parse_value as strtod;
    for (key, value) in fields {
        let g = &mut ent.game;
        match key.to_ascii_lowercase().as_str() {
            "classname" => g.classname = value.clone(),
            "model" => g.model = value.clone(),
            "spawnflags" => g.spawnflags = strtol(value),
            "speed" => g.speed = strtod(value),
            "accel" => g.accel = strtod(value),
            "decel" => g.decel = strtod(value),
            "target" => g.target = value.clone(),
            "targetname" => g.targetname = value.clone(),
            "pathtarget" => g.pathtarget = value.clone(),
            "killtarget" => g.killtarget = value.clone(),
            "deathtarget" => g.deathtarget = value.clone(),
            "combattarget" => g.combattarget = value.clone(),
            "message" => g.message = value.clone(),
            "team" => g.team = value.clone(),
            "map" => g.map = value.clone(),
            "wait" => g.wait = strtod(value),
            "delay" => g.delay = strtod(value),
            "random" => g.random = strtod(value),
            "move_origin" => g.move_origin = scan_vector(value),
            "move_angles" => g.move_angles = scan_vector(value),
            "style" => g.style = strtol(value),
            "count" => g.count = strtol(value),
            "health" => g.health = strtol(value),
            "dmg" => g.dmg = strtol(value),
            "mass" => g.mass = strtol(value),
            "volume" => g.volume = strtod(value),
            "attenuation" => g.attenuation = strtod(value),
            "origin" => ent.state.origin = scan_vector(value),
            "angles" => ent.state.angles = scan_vector(value),
            // F_ANGLEHACK: a single yaw.
            "angle" => ent.state.angles = q2_shared::types::Vec3f::new(0.0, strtod(value), 0.0),
            _ => {}
        }
    }
}

/// Spawn function for `info_player_start`: a marker; origin and angles
/// come from `apply_spawn_fields`. C also schedules SP_CreateUnnamedSpawn,
/// a fix-up for a few retail maps that is not ported.
pub fn sp_info_player_start(
    storage: &mut EntityStorage,
    key: EntityKey,
    _props: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "info_player_start".to_string();
    }
}

/// Spawn function for `info_player_deathmatch`. C frees these outside
/// deathmatch; this port keeps them because the single-player spawn point
/// search (player/client.rs) falls back to them on deathmatch-only maps.
pub fn sp_info_player_deathmatch(
    storage: &mut EntityStorage,
    key: EntityKey,
    _props: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "info_player_deathmatch".to_string();
    }
}

/// Spawn function for `info_player_coop`; [`info_player_coop_finish`]
/// removes it outside coop.
pub fn sp_info_player_coop(
    storage: &mut EntityStorage,
    key: EntityKey,
    _props: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "info_player_coop".to_string();
    }
}

/// Free a coop start outside coop (spawn functions cannot see the mode).
/// C's `SP_FixCoopSpots` fix-up for some retail maps is not ported.
/// C ref: `SP_info_player_coop`.
pub fn info_player_coop_finish(
    world: &mut GameWorld,
    key: EntityKey,
    _props: &HashMap<String, String>,
) {
    if !world.game.coop {
        world.free_entity(key);
    }
}

/// Spawn function for `info_player_intermission`: a camera marker with no
/// setup. C ref: `SP_info_player_intermission`.
pub fn sp_info_player_intermission(
    _storage: &mut EntityStorage,
    _key: EntityKey,
    _props: &HashMap<String, String>,
) {
}

/// Spawn function for `worldspawn`.
pub fn sp_worldspawn(storage: &mut EntityStorage, key: EntityKey, props: &HashMap<String, String>) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "worldspawn".to_string();
        if let Some(msg) = props.get("message") {
            ent.game.message = msg.clone();
        }
    }
}

/// Spawn function for `light`; `GameWorld::spawn_light` then frees it or
/// publishes its switchable style (C SP_light needs configstrings).
pub fn sp_light(storage: &mut EntityStorage, key: EntityKey, _props: &HashMap<String, String>) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "light".to_string();
    }
}

// ---------------------------------------------------------------------------
// Player spawn point finder
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_entity_string_basic() {
        let input = r#"
{
"classname" "info_player_start"
"origin" "0 0 0"
"angle" "90"
}
"#;
        let entities = parse_entity_string(input).unwrap();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0]["classname"], "info_player_start");
        assert_eq!(entities[0]["origin"], "0 0 0");
        assert_eq!(entities[0]["angle"], "90");
    }

    #[test]
    fn parse_entity_string_multiple() {
        let input = r#"
{
"classname" "worldspawn"
"message" "Test Map"
}
{
"classname" "info_player_start"
"origin" "128 64 0"
}
{
"classname" "light"
"origin" "256 256 128"
"_color" "1 1 1"
}
"#;
        let entities = parse_entity_string(input).unwrap();
        assert_eq!(entities.len(), 3);

        assert_eq!(entities[0]["classname"], "worldspawn");
        assert_eq!(entities[0]["message"], "Test Map");

        assert_eq!(entities[1]["classname"], "info_player_start");
        assert_eq!(entities[1]["origin"], "128 64 0");

        assert_eq!(entities[2]["classname"], "light");
        // _color should have been discarded (editor-only key).
        assert!(!entities[2].contains_key("_color"));
    }

    #[test]
    fn parse_entity_string_empty() {
        let entities = parse_entity_string("").unwrap();
        assert!(entities.is_empty());
    }

    #[test]
    fn parse_rejects_entity_without_closing_brace() {
        // C ED_ParseEdict: "EOF without closing brace".
        assert!(parse_entity_string(r#"{ "classname" "worldspawn""#).is_err());
        assert!(parse_entity_string(r#"{ "classname" "worldspawn" "message""#).is_err());
    }

    #[test]
    fn parse_rejects_key_followed_by_closing_brace() {
        // C ED_ParseEdict: "closing brace without data".
        assert!(parse_entity_string(r#"{ "classname" }"#).is_err());
    }

    #[test]
    fn parse_rejects_text_where_opening_brace_expected() {
        // C SpawnEntities: "found %s when expecting {".
        assert!(parse_entity_string(r#"garbage { "classname" "light" }"#).is_err());
        assert!(parse_entity_string(r#"{ "classname" "light" } stray"#).is_err());
    }

    #[test]
    fn parse_reads_unquoted_tokens_like_com_parse() {
        let entities = parse_entity_string("{ classname light origin \"1 2 3\" }").unwrap();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0]["classname"], "light");
        assert_eq!(entities[0]["origin"], "1 2 3");
    }

    #[test]
    fn parse_entity_string_backslash_n() {
        let input = r#"
{
"classname" "worldspawn"
"message" "line1\nline2"
}
"#;
        let entities = parse_entity_string(input).unwrap();
        assert_eq!(entities[0]["message"], "line1\nline2");
    }

    #[test]
    fn spawn_table_lookup() {
        let table = SpawnTable::new();

        assert!(table.get("info_player_start").is_some());
        assert!(table.get("info_player_deathmatch").is_some());
        assert!(table.get("worldspawn").is_some());
        assert!(table.get("light").is_some());
        assert!(table.get("nonexistent_entity").is_none());
    }

    #[test]
    fn spawn_function_sets_entity_data() {
        let mut storage = EntityStorage::new(64);
        let key = storage.spawn().unwrap();

        let mut props = HashMap::new();
        props.insert("origin".to_string(), "100 200 300".to_string());
        props.insert("angle".to_string(), "45".to_string());

        apply_spawn_fields(storage.get_mut(key).unwrap(), &props);
        sp_info_player_start(&mut storage, key, &props);

        let ent = storage.get(key).unwrap();
        assert_eq!(ent.game.classname, "info_player_start");
        assert_eq!(ent.state.origin.x, 100.0);
        assert_eq!(ent.state.origin.y, 200.0);
        assert_eq!(ent.state.origin.z, 300.0);
        assert_eq!(ent.state.angles.y, 45.0);
    }
    #[test]
    fn backslash_before_other_characters_becomes_a_lone_backslash() {
        let entities = parse_entity_string(r#"{ "message" "a\qb\" }"#).unwrap();
        assert_eq!(entities[0]["message"], "a\\b\\");
    }

    #[test]
    fn player_start_keeps_the_angles_key_and_partial_origins() {
        let mut storage = EntityStorage::new(64);
        let key = storage.spawn().unwrap();
        let mut props = HashMap::new();
        props.insert("classname".to_string(), "info_player_start".to_string());
        props.insert("angles".to_string(), "0 90 0".to_string());
        props.insert("origin".to_string(), "10 20".to_string());
        apply_spawn_fields(storage.get_mut(key).unwrap(), &props);
        sp_info_player_start(&mut storage, key, &props);
        let ent = storage.get(key).unwrap();
        assert_eq!(ent.state.angles.y, 90.0);
        assert_eq!(ent.state.origin.x, 10.0);
        assert_eq!(ent.state.origin.y, 20.0);
    }

    #[test]
    fn c_spawn_names_have_spawn_functions_or_models() {
        let table = SpawnTable::new();
        assert!(table.get("info_player_coop").is_some());
        assert!(table.get("info_player_intermission").is_some());
        assert!(find_spawn("monster_berserk").is_some_and(|e| e.2.is_some()));
        assert!(find_spawn("monster_berserker").is_none());
        assert!(find_spawn("misc_insane").is_some_and(|e| e.2.is_some()));
        assert!(find_spawn("monster_insane").is_none());
    }

    /// C `spawns[]` (g_spawn.c), in order.
    const C_SPAWNS: &[&str] = &[
        "item_health",
        "item_health_small",
        "item_health_large",
        "item_health_mega",
        "info_player_start",
        "info_player_deathmatch",
        "info_player_coop",
        "info_player_intermission",
        "func_plat",
        "func_button",
        "func_door",
        "func_door_secret",
        "func_door_rotating",
        "func_rotating",
        "func_train",
        "func_water",
        "func_conveyor",
        "func_areaportal",
        "func_clock",
        "func_wall",
        "func_object",
        "func_timer",
        "func_explosive",
        "func_killbox",
        "trigger_always",
        "trigger_once",
        "trigger_multiple",
        "trigger_relay",
        "trigger_push",
        "trigger_hurt",
        "trigger_key",
        "trigger_counter",
        "trigger_elevator",
        "trigger_gravity",
        "trigger_monsterjump",
        "target_temp_entity",
        "target_speaker",
        "target_explosion",
        "target_changelevel",
        "target_secret",
        "target_goal",
        "target_splash",
        "target_spawner",
        "target_blaster",
        "target_crosslevel_trigger",
        "target_crosslevel_target",
        "target_laser",
        "target_help",
        "target_lightramp",
        "target_earthquake",
        "target_character",
        "target_string",
        "worldspawn",
        "viewthing",
        "light",
        "light_mine1",
        "light_mine2",
        "info_null",
        "func_group",
        "info_notnull",
        "path_corner",
        "point_combat",
        "misc_explobox",
        "misc_banner",
        "misc_satellite_dish",
        "misc_gib_arm",
        "misc_gib_leg",
        "misc_gib_head",
        "misc_insane",
        "misc_deadsoldier",
        "misc_viper",
        "misc_viper_bomb",
        "misc_bigviper",
        "misc_strogg_ship",
        "misc_teleporter",
        "misc_teleporter_dest",
        "misc_blackhole",
        "misc_eastertank",
        "misc_easterchick",
        "misc_easterchick2",
        "monster_berserk",
        "monster_gladiator",
        "monster_gunner",
        "monster_infantry",
        "monster_soldier_light",
        "monster_soldier",
        "monster_soldier_ss",
        "monster_tank",
        "monster_tank_commander",
        "monster_medic",
        "monster_flipper",
        "monster_chick",
        "monster_parasite",
        "monster_flyer",
        "monster_brain",
        "monster_floater",
        "monster_hover",
        "monster_mutant",
        "monster_supertank",
        "monster_boss2",
        "monster_boss3_stand",
        "monster_makron",
        "monster_jorg",
        "monster_commander_body",
        "turret_breach",
        "turret_base",
        "turret_driver",
    ];

    /// C `spawns[]` rows `SPAWNS` lacks: the health spawns take the item
    /// path ahead of the table (`items::health_spawn`).
    const NOT_IN_SPAWNS: &[&str] = &[
        "item_health",
        "item_health_small",
        "item_health_large",
        "item_health_mega",
    ];

    #[test]
    fn spawn_table_lists_the_c_spawns_in_c_order() {
        let expected: Vec<&str> = C_SPAWNS
            .iter()
            .copied()
            .filter(|c| !NOT_IN_SPAWNS.contains(c))
            .collect();
        let names: Vec<&str> = SPAWNS.iter().map(|e| e.0).collect();
        assert_eq!(names, expected);
    }

    #[test]
    fn every_spawn_entry_spawns_on_a_synthetic_world_with_its_model() {
        for &(classname, _, model, _) in SPAWNS {
            let (mut world, gi) = crate::world::test_world_recording();
            world.init_game();
            let ents = match classname {
                "worldspawn" => "{ \"classname\" \"worldspawn\" }".to_string(),
                _ => format!(
                    "{{ \"classname\" \"worldspawn\" }}\n\
                     {{ \"classname\" \"{classname}\" \"targetname\" \"t\" \"target\" \"t\" }}"
                ),
            };
            world.spawn_entities("t", &ents, "");
            if let Some(model) = model {
                assert!(gi.models.borrow().iter().any(|m| m == model), "{classname}");
            }
        }
    }

    #[test]
    fn easy_and_hard_skills_drop_only_entities_flagged_not_for_them() {
        // C ED_LoadFromFile: skill 0 drops SPAWNFLAG_NOT_EASY (256),
        // skill 2 and up drop SPAWNFLAG_NOT_HARD (1024).
        for (skill, dropped, kept) in [(0, "not_easy", "not_hard"), (2, "not_hard", "not_easy")] {
            let mut world = crate::world::test_world();
            world.init_game();
            world.game.skill = skill;
            world.spawn_entities(
                "base1",
                concat!(
                    "{ \"classname\" \"worldspawn\" }\n",
                    "{ \"classname\" \"info_notnull\" \"targetname\" \"not_easy\" \"spawnflags\" \"256\" }\n",
                    "{ \"classname\" \"info_notnull\" \"targetname\" \"not_hard\" \"spawnflags\" \"1024\" }\n",
                ),
                "",
            );
            assert!(
                world.find_by_targetname(None, dropped).is_none(),
                "skill {skill}"
            );
            assert!(
                world.find_by_targetname(None, kept).is_some(),
                "skill {skill}"
            );
        }
    }
}
