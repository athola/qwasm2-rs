//! Entity spawning — entity-string parser and classname-to-function dispatch.
//!
//! Mirrors the C `g_spawn.c` spawn table and `ED_ParseEdict` parser.

use std::collections::HashMap;

use q2_common::error::{Q2Error, Q2Result};

use crate::entity::{EntityKey, EntityStorage};

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
                // Handle backslash-n from the original C code.
                if chars.peek() == Some(&'n') {
                    chars.next();
                    s.push('\n');
                } else {
                    s.push('\\');
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

/// Registry mapping classnames to spawn functions.
pub struct SpawnTable {
    table: HashMap<String, SpawnFn>,
}

impl SpawnTable {
    /// Build the default spawn table with the basic entity types.
    pub fn new() -> Self {
        let mut table = HashMap::new();

        table.insert(
            "info_player_start".to_string(),
            sp_info_player_start as SpawnFn,
        );
        table.insert(
            "info_player_deathmatch".to_string(),
            sp_info_player_deathmatch as SpawnFn,
        );
        table.insert("worldspawn".to_string(), sp_worldspawn as SpawnFn);
        table.insert("light".to_string(), sp_light as SpawnFn);

        Self { table }
    }

    /// Look up the spawn function for a given classname.
    pub fn get(&self, classname: &str) -> Option<&SpawnFn> {
        self.table.get(classname)
    }

    /// Register an additional spawn function.
    pub fn insert(&mut self, classname: impl Into<String>, func: SpawnFn) {
        self.table.insert(classname.into(), func);
    }
}

impl Default for SpawnTable {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Placeholder spawn functions
// ---------------------------------------------------------------------------

/// Parse an "origin" string like `"128 64 0"` into a `Vec3f`.
/// Returns `None` unless the string is exactly three numbers, so a real
/// `"0 0 0"` origin is distinguishable from a malformed one.
pub fn parse_origin(s: &str) -> Option<q2_shared::types::Vec3f> {
    let parts: Result<Vec<f32>, _> = s.split_whitespace().map(str::parse).collect();
    match parts.as_deref() {
        Ok([x, y, z]) => Some(q2_shared::types::Vec3f::new(*x, *y, *z)),
        _ => {
            tracing::debug!("malformed origin string: {:?}", s);
            None
        }
    }
}

/// The entity's `origin` key, or `None` when missing or malformed.
fn origin_from_props(props: &HashMap<String, String>) -> Option<q2_shared::types::Vec3f> {
    props.get("origin").and_then(|s| parse_origin(s))
}

/// C `strtol(s, NULL, 10)`: optional sign and leading digits, 0 if none.
fn strtol(s: &str) -> i32 {
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
fn scan_vector(s: &str) -> q2_shared::types::Vec3f {
    let mut v = [0.0f32; 3];
    for (slot, part) in v.iter_mut().zip(s.split_whitespace()) {
        *slot = q2_common::cvar::parse_value(part);
    }
    q2_shared::types::Vec3f::from_array(v)
}

/// Set the generic entity fields from an entity-string block, as C
/// ED_ParseEdict does for every entity before its spawn function runs.
/// Fields this port has no slot for yet (`model`, `sounds`, `deathtarget`,
/// `map`, the spawn-temp keys) are left to the spawn functions.
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
            "combattarget" => g.combattarget = value.clone(),
            "message" => g.message = value.clone(),
            "team" => g.team = value.clone(),
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

/// Parse an "angle" string into entity angles (Quake convention: yaw only).
fn parse_angle(props: &HashMap<String, String>) -> q2_shared::types::Vec3f {
    if let Some(angle_str) = props.get("angle") {
        if let Ok(yaw) = angle_str.parse::<f32>() {
            return q2_shared::types::Vec3f::new(0.0, yaw, 0.0);
        }
        tracing::debug!("malformed angle string: {:?}", angle_str);
    }
    q2_shared::types::Vec3f::ZERO
}

/// Spawn function for `info_player_start`.
pub fn sp_info_player_start(
    storage: &mut EntityStorage,
    key: EntityKey,
    props: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "info_player_start".to_string();
        ent.state.origin = origin_from_props(props).unwrap_or_default();
        ent.state.angles = parse_angle(props);
    }
}

/// Spawn function for `info_player_deathmatch`.
pub fn sp_info_player_deathmatch(
    storage: &mut EntityStorage,
    key: EntityKey,
    props: &HashMap<String, String>,
) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "info_player_deathmatch".to_string();
        ent.state.origin = origin_from_props(props).unwrap_or_default();
        ent.state.angles = parse_angle(props);
    }
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

/// Spawn function for `light` — lights are stripped at runtime (no entity
/// needed), but we record the classname so the spawn system doesn't warn.
pub fn sp_light(storage: &mut EntityStorage, key: EntityKey, props: &HashMap<String, String>) {
    if let Some(ent) = storage.get_mut(key) {
        ent.game.classname = "light".to_string();
        ent.state.origin = origin_from_props(props).unwrap_or_default();
        // Lights have no runtime behaviour — entity can be freed later.
        let _ = props;
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
    fn spawn_table_custom_insert() {
        let mut table = SpawnTable::new();

        fn sp_custom(
            storage: &mut EntityStorage,
            key: EntityKey,
            _props: &HashMap<String, String>,
        ) {
            if let Some(ent) = storage.get_mut(key) {
                ent.game.classname = "custom".to_string();
            }
        }

        table.insert("custom", sp_custom);
        assert!(table.get("custom").is_some());
    }

    #[test]
    fn spawn_function_sets_entity_data() {
        let mut storage = EntityStorage::new(64);
        let key = storage.spawn().unwrap();

        let mut props = HashMap::new();
        props.insert("origin".to_string(), "100 200 300".to_string());
        props.insert("angle".to_string(), "45".to_string());

        sp_info_player_start(&mut storage, key, &props);

        let ent = storage.get(key).unwrap();
        assert_eq!(ent.game.classname, "info_player_start");
        assert_eq!(ent.state.origin.x, 100.0);
        assert_eq!(ent.state.origin.y, 200.0);
        assert_eq!(ent.state.origin.z, 300.0);
        assert_eq!(ent.state.angles.y, 45.0);
    }
}
