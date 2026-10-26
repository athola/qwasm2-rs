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
///
/// Returns `None` when the key is missing or is not exactly three numbers,
/// so a real `"0 0 0"` origin is distinguishable from a malformed one.
fn parse_origin(props: &HashMap<String, String>) -> Option<q2_shared::types::Vec3f> {
    let origin_str = props.get("origin")?;
    let parts: Result<Vec<f32>, _> = origin_str.split_whitespace().map(str::parse).collect();
    match parts.as_deref() {
        Ok([x, y, z]) => Some(q2_shared::types::Vec3f::new(*x, *y, *z)),
        _ => {
            tracing::debug!("malformed origin string: {:?}", origin_str);
            None
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
        ent.state.origin = parse_origin(props).unwrap_or_default();
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
        ent.state.origin = parse_origin(props).unwrap_or_default();
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
        ent.state.origin = parse_origin(props).unwrap_or_default();
        // Lights have no runtime behaviour — entity can be freed later.
        let _ = props;
    }
}

// ---------------------------------------------------------------------------
// Player spawn point finder
// ---------------------------------------------------------------------------

/// Find the player spawn point in a BSP entity string.
///
/// Follows the single-player branch of C `SelectSpawnPoint`
/// (game/player/client.c): with an empty `spawnpoint`, the first
/// `info_player_start` without a `targetname`, else any `info_player_start`;
/// with a `spawnpoint`, only the `info_player_start` whose `targetname`
/// matches it case-insensitively (level transitions). C errors when nothing
/// matches; this returns `None`.
///
/// Prototype extension: the standalone WASM build has no deathmatch/coop
/// mode, so a map without any `info_player_start` falls back to the first
/// `info_player_deathmatch`, then `info_player_coop` (the classes C picks
/// in those modes).
///
/// Returns `(origin, yaw_angle)`. Spawn entities with a malformed origin
/// are skipped.
pub fn find_player_start(
    entstring: &str,
    spawnpoint: &str,
) -> Option<(q2_shared::types::Vec3f, f32)> {
    struct SpawnCandidate<'a> {
        classname: &'a str,
        targetname: Option<&'a str>,
        origin: q2_shared::types::Vec3f,
        angle: f32,
    }

    let entities = match parse_entity_string(entstring) {
        Ok(entities) => entities,
        Err(e) => {
            tracing::error!("find_player_start: {e}");
            return None;
        }
    };
    let spawns: Vec<SpawnCandidate<'_>> = entities
        .iter()
        .filter_map(|ent| {
            Some(SpawnCandidate {
                classname: ent.get("classname")?,
                targetname: ent.get("targetname").map(String::as_str),
                origin: parse_origin(ent)?,
                angle: ent.get("angle").and_then(|s| s.parse().ok()).unwrap_or(0.0),
            })
        })
        .collect();
    let of_class = |class: &'static str| spawns.iter().filter(move |s| s.classname == class);

    let chosen = if spawnpoint.is_empty() {
        of_class("info_player_start")
            .find(|s| s.targetname.is_none())
            .or_else(|| of_class("info_player_start").next())
            .or_else(|| of_class("info_player_deathmatch").next())
            .or_else(|| of_class("info_player_coop").next())
    } else {
        of_class("info_player_start").find(|s| {
            s.targetname
                .is_some_and(|t| t.eq_ignore_ascii_case(spawnpoint))
        })
    };
    chosen.map(|s| (s.origin, s.angle))
}

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

    #[test]
    fn find_player_start_prefers_unnamed() {
        let input = r#"
{
"classname" "info_player_start"
"origin" "100 200 300"
"targetname" "transition_spawn"
"angle" "45"
}
{
"classname" "info_player_start"
"origin" "0 0 0"
"angle" "90"
}
"#;
        let (origin, angle) = find_player_start(input, "").unwrap();
        // Should pick the unnamed one
        assert_eq!(origin, q2_shared::types::Vec3f::new(0.0, 0.0, 0.0));
        assert_eq!(angle, 90.0);
    }

    #[test]
    fn find_player_start_falls_back_to_named() {
        let input = r#"
{
"classname" "info_player_start"
"origin" "100 200 300"
"targetname" "transition_spawn"
"angle" "45"
}
"#;
        let (origin, angle) = find_player_start(input, "").unwrap();
        assert_eq!(origin, q2_shared::types::Vec3f::new(100.0, 200.0, 300.0));
        assert_eq!(angle, 45.0);
    }

    #[test]
    fn find_player_start_priority_order() {
        let input = r#"
{
"classname" "info_player_deathmatch"
"origin" "50 50 50"
}
{
"classname" "info_player_start"
"origin" "100 100 100"
}
"#;
        let (origin, _) = find_player_start(input, "").unwrap();
        // info_player_start has higher priority than deathmatch
        assert_eq!(origin, q2_shared::types::Vec3f::new(100.0, 100.0, 100.0));
    }

    #[test]
    fn find_player_start_none_when_empty() {
        assert!(find_player_start("", "").is_none());
    }

    #[test]
    fn find_player_start_none_when_no_origin() {
        let input = r#"
{
"classname" "worldspawn"
"message" "Test Map"
}
"#;
        assert!(find_player_start(input, "").is_none());
    }

    #[test]
    fn find_player_start_accepts_zero_origin_written_as_floats() {
        let input = r#"
{
"classname" "info_player_start"
"origin" "0 0 0.0"
"angle" "180"
}
"#;
        let (origin, angle) = find_player_start(input, "").unwrap();
        assert_eq!(origin, q2_shared::types::Vec3f::ZERO);
        assert_eq!(angle, 180.0);
    }

    #[test]
    fn find_player_start_skips_origin_with_non_numeric_token() {
        let input = r#"
{
"classname" "info_player_start"
"origin" "1 x 3 4"
}
"#;
        assert!(find_player_start(input, "").is_none());
    }

    #[test]
    fn find_player_start_ignores_non_spawn_entities() {
        // C SelectSpawnPoint never spawns the player at a light.
        let input = r#"
{
"classname" "light"
"origin" "64 64 64"
}
{
"classname" "info_player_intermission"
"origin" "32 32 32"
}
"#;
        assert!(find_player_start(input, "").is_none());
    }

    #[test]
    fn find_player_start_selects_spawn_matching_spawnpoint() {
        let input = r#"
{
"classname" "info_player_start"
"origin" "0 0 0"
}
{
"classname" "info_player_start"
"origin" "100 200 300"
"targetname" "From_Base2"
"angle" "45"
}
"#;
        // Q_stricmp: the match is case-insensitive.
        let (origin, angle) = find_player_start(input, "from_base2").unwrap();
        assert_eq!(origin, q2_shared::types::Vec3f::new(100.0, 200.0, 300.0));
        assert_eq!(angle, 45.0);
    }

    #[test]
    fn find_player_start_none_when_spawnpoint_has_no_match() {
        // C: gi.error("Couldn't find spawn point %s").
        let input = r#"
{
"classname" "info_player_start"
"origin" "0 0 0"
}
"#;
        assert!(find_player_start(input, "missing").is_none());
    }

    #[test]
    fn find_player_start_uses_deathmatch_spawn_when_map_has_no_player_start() {
        let input = r#"
{
"classname" "info_player_deathmatch"
"origin" "8 16 24"
"angle" "270"
}
"#;
        let (origin, angle) = find_player_start(input, "").unwrap();
        assert_eq!(origin, q2_shared::types::Vec3f::new(8.0, 16.0, 24.0));
        assert_eq!(angle, 270.0);
    }
}
