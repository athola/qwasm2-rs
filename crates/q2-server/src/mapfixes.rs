//! yquake2's fixed entity files for the retail maps (`stuff/mapfixes`),
//! shipped in the server as yquake2 ships them beside its engine. Each
//! file's header lists its fixes: monster counts, broken spawn chains,
//! deathmatch items in single player, and unusable entities such as
//! biggun's `target_actor` (no actor code exists in yquake2).

/// The maps with a fix.
pub const MAPS: [&str; 12] = [
    "base2", "base3", "biggun", "city1", "city2", "city3", "cool1", "hangar1", "jail5", "lab",
    "train", "waste3",
];

/// `maps/<map>.ent`, the entity string C CMod_LoadEntityString loads in
/// place of the map's own when `sv_entfile` is set.
pub fn entity_file(map: &str) -> Option<&'static str> {
    Some(match map {
        "base2" => include_str!("../mapfixes/baseq2/base2.ent"),
        "base3" => include_str!("../mapfixes/baseq2/base3.ent"),
        "biggun" => include_str!("../mapfixes/baseq2/biggun.ent"),
        "city1" => include_str!("../mapfixes/baseq2/city1.ent"),
        "city2" => include_str!("../mapfixes/baseq2/city2.ent"),
        "city3" => include_str!("../mapfixes/baseq2/city3.ent"),
        "cool1" => include_str!("../mapfixes/baseq2/cool1.ent"),
        "hangar1" => include_str!("../mapfixes/baseq2/hangar1.ent"),
        "jail5" => include_str!("../mapfixes/baseq2/jail5.ent"),
        "lab" => include_str!("../mapfixes/baseq2/lab.ent"),
        "train" => include_str!("../mapfixes/baseq2/train.ent"),
        "waste3" => include_str!("../mapfixes/baseq2/waste3.ent"),
        _ => return None,
    })
}
