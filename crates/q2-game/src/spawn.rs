//! Entity spawning — entity-string parser and classname-to-function dispatch.
//!
//! Mirrors the C `g_spawn.c` spawn table and `ED_ParseEdict` parser.

use std::collections::HashMap;

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
/// Keys starting with `_` are editor-only comments and are discarded.
pub fn parse_entity_string(entstring: &str) -> Vec<HashMap<String, String>> {
    let mut result = Vec::new();
    let mut chars = entstring.chars().peekable();

    loop {
        // Skip whitespace looking for '{'
        skip_whitespace(&mut chars);
        match chars.peek() {
            Some('{') => {
                chars.next(); // consume '{'
            }
            _ => break, // end of string or unexpected — done
        }

        let mut entity: HashMap<String, String> = HashMap::new();

        loop {
            skip_whitespace(&mut chars);
            match chars.peek() {
                Some('}') => {
                    chars.next(); // consume '}'
                    break;
                }
                Some('"') => {
                    let key = parse_quoted_string(&mut chars);
                    skip_whitespace(&mut chars);
                    let value = parse_quoted_string(&mut chars);

                    // Discard editor-only keys (leading underscore).
                    if !key.starts_with('_') {
                        entity.insert(key, value);
                    }
                }
                Some(_) => {
                    // Token without quotes (rare but tolerated) — skip it.
                    chars.next();
                }
                None => break, // premature EOF
            }
        }

        if !entity.is_empty() {
            result.push(entity);
        }
    }

    result
}

/// Advance past whitespace characters.
fn skip_whitespace(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    while let Some(&c) = chars.peek() {
        if c.is_ascii_whitespace() {
            chars.next();
        } else {
            break;
        }
    }
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
fn parse_origin(props: &HashMap<String, String>) -> q2_shared::types::Vec3f {
    if let Some(origin_str) = props.get("origin") {
        let parts: Vec<f32> = origin_str
            .split_whitespace()
            .filter_map(|s| s.parse::<f32>().ok())
            .collect();
        if parts.len() == 3 {
            return q2_shared::types::Vec3f::new(parts[0], parts[1], parts[2]);
        }
        tracing::debug!("malformed origin string: {:?}", origin_str);
    }
    q2_shared::types::Vec3f::ZERO
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
        ent.state.origin = parse_origin(props);
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
        ent.state.origin = parse_origin(props);
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
        ent.state.origin = parse_origin(props);
        // Lights have no runtime behaviour — entity can be freed later.
        let _ = props;
    }
}

// ---------------------------------------------------------------------------
// Player spawn point finder
// ---------------------------------------------------------------------------

/// Player spawn classnames in priority order.
const SPAWN_CLASSNAMES: &[&str] = &[
    "info_player_start",
    "info_player_deathmatch",
    "info_player_coop",
    "info_player_intermission",
    "misc_teleporter_dest",
];

/// Find the best player spawn point from a BSP entity string.
///
/// Prefers unnamed `info_player_start` (the default spawn when starting
/// a map fresh). Named spawns (with `targetname`) are used for level
/// transitions and are lower priority.
///
/// Returns `(origin, yaw_angle)` or `None` if no spawn entity found.
pub fn find_player_start(entstring: &str) -> Option<(q2_shared::types::Vec3f, f32)> {
    let entities = parse_entity_string(entstring);

    // Collect spawn candidates
    struct SpawnCandidate {
        classname: String,
        origin: q2_shared::types::Vec3f,
        targetname: String,
        angle: f32,
    }

    let mut spawns: Vec<SpawnCandidate> = Vec::new();

    for ent in &entities {
        let classname = match ent.get("classname") {
            Some(c) => c.clone(),
            None => continue,
        };
        // Reuse parse_origin; skip entities that have no "origin" key at all.
        if !ent.contains_key("origin") {
            continue;
        }
        let origin = parse_origin(ent);
        if origin == q2_shared::types::Vec3f::ZERO
            && ent.get("origin").is_none_or(|s| s.trim() != "0 0 0")
        {
            continue; // malformed origin (parse_origin returned zero as fallback)
        }
        let targetname = ent.get("targetname").cloned().unwrap_or_default();
        let angle = ent.get("angle").and_then(|s| s.parse().ok()).unwrap_or(0.0);

        spawns.push(SpawnCandidate {
            classname,
            origin,
            targetname,
            angle,
        });
    }

    // Find best spawn by priority: prefer unnamed spawns first
    for target in SPAWN_CLASSNAMES {
        if let Some(s) = spawns
            .iter()
            .find(|s| s.classname == *target && s.targetname.is_empty())
        {
            return Some((s.origin, s.angle));
        }
        if let Some(s) = spawns.iter().find(|s| s.classname == *target) {
            return Some((s.origin, s.angle));
        }
    }

    // Fallback: any entity with an origin
    spawns.first().map(|s| (s.origin, s.angle))
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
        let entities = parse_entity_string(input);
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
        let entities = parse_entity_string(input);
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
        let entities = parse_entity_string("");
        assert!(entities.is_empty());
    }

    #[test]
    fn parse_entity_string_backslash_n() {
        let input = r#"
{
"classname" "worldspawn"
"message" "line1\nline2"
}
"#;
        let entities = parse_entity_string(input);
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
        let (origin, angle) = find_player_start(input).unwrap();
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
        let (origin, angle) = find_player_start(input).unwrap();
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
        let (origin, _) = find_player_start(input).unwrap();
        // info_player_start has higher priority than deathmatch
        assert_eq!(origin, q2_shared::types::Vec3f::new(100.0, 100.0, 100.0));
    }

    #[test]
    fn find_player_start_none_when_empty() {
        assert!(find_player_start("").is_none());
    }

    #[test]
    fn find_player_start_none_when_no_origin() {
        let input = r#"
{
"classname" "worldspawn"
"message" "Test Map"
}
"#;
        assert!(find_player_start(input).is_none());
    }
}
