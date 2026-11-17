//! yquake2's fixed entity files for the retail maps (`stuff/mapfixes`),
//! shipped in the server as yquake2 ships them beside its engine. Each
//! file's header lists its fixes: monster counts, broken spawn chains,
//! deathmatch items in single player, and unusable entities such as
//! biggun's `target_actor` (no actor code exists in yquake2).

macro_rules! fixes {
    ($($map:literal),*) => {
        [$(($map, include_str!(concat!("../mapfixes/baseq2/", $map, ".ent")))),*]
    };
}

/// Each fixed map and its `maps/<map>.ent`.
pub const FIXES: [(&str, &str); 12] = fixes!(
    "base2", "base3", "biggun", "city1", "city2", "city3", "cool1", "hangar1", "jail5", "lab",
    "train", "waste3"
);

/// `maps/<map>.ent`, the entity string C CMod_LoadEntityString loads in
/// place of the map's own when `sv_entfile` is set.
pub fn entity_file(map: &str) -> Option<&'static str> {
    FIXES
        .iter()
        .find(|(name, _)| *name == map)
        .map(|(_, entities)| *entities)
}
