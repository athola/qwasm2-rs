//! Spatial partitioning for entity queries.
//!
//! Rust equivalent of the area-node BSP from `sv_world.c`
//! (C reference: Qwasm2/src/server/sv_world.c).
//!
//! The C code uses a fixed array of 32 `areanode_t` nodes organized as a
//! binary tree. Each internal node splits along the axis with the largest
//! extent, and each node carries two linked lists: one for trigger entities
//! and one for solid entities.
//!
//! This Rust version uses `Vec`-based lists instead of intrusive linked
//! lists, and indices instead of raw pointers.

use q2_shared::types::{Solid, Vec3f};

/// Depth limit when building the area tree (matches `AREA_DEPTH` in C).
const AREA_DEPTH: usize = 4;

/// Area type constants used by `area_edicts()`.
pub const AREA_SOLID: i32 = 1;
pub const AREA_TRIGGERS: i32 = 2;

// ---------------------------------------------------------------------------
// AreaNode
// ---------------------------------------------------------------------------

/// A single node in the spatial partitioning tree.
///
/// Corresponds to `areanode_t` from the C codebase. Internal nodes split the
/// world along `axis` (0 = X, 1 = Y) at position `dist`. Leaf nodes have
/// `axis == -1`.
#[derive(Debug, Clone)]
pub struct AreaNode {
    /// Split axis: 0 = X, 1 = Y, -1 = leaf node.
    pub axis: i32,
    /// Split position along `axis`.
    pub dist: f32,
    /// Indices into the parent `ServerWorld.area_nodes` vec. `[0]` is the
    /// "greater" child, `[1]` is the "lesser" child. `usize::MAX` means
    /// no child (leaf).
    pub children: [usize; 2],
    /// Entity indices linked as triggers in this node.
    pub trigger_edicts: Vec<usize>,
    /// Entity indices linked as solids in this node.
    pub solid_edicts: Vec<usize>,
}

impl Default for AreaNode {
    fn default() -> Self {
        Self {
            axis: -1,
            dist: 0.0,
            children: [usize::MAX; 2],
            trigger_edicts: Vec::new(),
            solid_edicts: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Linked entity record
// ---------------------------------------------------------------------------

/// Bookkeeping for a linked entity so we can unlink it later.
#[derive(Debug, Clone)]
struct LinkedEntity {
    /// Which area node the entity lives in.
    node_idx: usize,
    /// Whether it was inserted into the trigger list (`true`) or the solid
    /// list (`false`).
    is_trigger: bool,
    /// Axis-aligned bounding box at the time of linking.
    mins: Vec3f,
    maxs: Vec3f,
}

// ---------------------------------------------------------------------------
// ServerWorld
// ---------------------------------------------------------------------------

/// The server-side spatial partitioning structure.
///
/// Owns the area-node tree and the set of currently linked entities.
pub struct ServerWorld {
    area_nodes: Vec<AreaNode>,
    /// Per-entity link info, keyed by entity index. `None` means the entity
    /// is not currently linked.
    links: Vec<Option<LinkedEntity>>,
}

impl std::fmt::Debug for ServerWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerWorld")
            .field("area_nodes", &self.area_nodes.len())
            .field("links", &self.links.len())
            .finish()
    }
}

impl ServerWorld {
    /// Create a new, empty world. Call `clear()` with the world bounds after
    /// loading a map to build the area tree.
    pub fn new() -> Self {
        Self {
            area_nodes: Vec::new(),
            links: Vec::new(),
        }
    }

    /// Reset the world and rebuild the area tree for the given bounds.
    ///
    /// Corresponds to `SV_ClearWorld()` in the C codebase.
    pub fn clear(&mut self, world_mins: Vec3f, world_maxs: Vec3f) {
        self.area_nodes.clear();
        self.links.clear();
        self.build_tree(0, world_mins, world_maxs);
    }

    // -- internal: recursive tree builder -----------------------------------

    fn build_tree(&mut self, depth: usize, mins: Vec3f, maxs: Vec3f) -> usize {
        let idx = self.area_nodes.len();
        self.area_nodes.push(AreaNode::default());

        if depth == AREA_DEPTH {
            // Leaf node — axis stays -1.
            return idx;
        }

        let size = maxs - mins;
        let axis: usize = if size.x > size.y { 0 } else { 1 };
        let dist = 0.5 * (Self::component(maxs, axis) + Self::component(mins, axis));

        self.area_nodes[idx].axis = axis as i32;
        self.area_nodes[idx].dist = dist;

        // Child 0: the "greater" half (mins along axis = dist).
        let mut mins2 = mins;
        Self::set_component(&mut mins2, axis, dist);
        let child0 = self.build_tree(depth + 1, mins2, maxs);

        // Child 1: the "lesser" half (maxs along axis = dist).
        let mut maxs1 = maxs;
        Self::set_component(&mut maxs1, axis, dist);
        let child1 = self.build_tree(depth + 1, mins, maxs1);

        self.area_nodes[idx].children = [child0, child1];
        idx
    }

    // -- public API ---------------------------------------------------------

    /// Link an entity into the world.
    ///
    /// Corresponds to the tail end of `SV_LinkEdict()` in the C codebase
    /// (the part that inserts the entity into an area-node list).
    pub fn link_entity(&mut self, ent_idx: usize, mins: Vec3f, maxs: Vec3f, solid: Solid) {
        // Unlink first if already linked.
        self.unlink_entity(ent_idx);

        if solid == Solid::Not {
            return; // non-solid entities are not inserted into the tree.
        }

        if self.area_nodes.is_empty() {
            return; // no tree built yet.
        }

        let is_trigger = solid == Solid::Trigger;

        // Walk the tree to find the correct node.
        let mut node_idx: usize = 0;
        loop {
            let node = &self.area_nodes[node_idx];
            if node.axis == -1 {
                break; // leaf
            }
            let axis = node.axis as usize;
            let dist = node.dist;
            let children = node.children;

            if Self::component(mins, axis) > dist {
                node_idx = children[0];
            } else if Self::component(maxs, axis) < dist {
                node_idx = children[1];
            } else {
                break; // entity crosses the split plane
            }
        }

        // Insert into the appropriate list.
        if is_trigger {
            self.area_nodes[node_idx].trigger_edicts.push(ent_idx);
        } else {
            self.area_nodes[node_idx].solid_edicts.push(ent_idx);
        }

        // Ensure the links vec is large enough.
        if self.links.len() <= ent_idx {
            self.links.resize(ent_idx + 1, None);
        }
        self.links[ent_idx] = Some(LinkedEntity {
            node_idx,
            is_trigger,
            mins,
            maxs,
        });
    }

    /// Unlink an entity from the world.
    ///
    /// Corresponds to `SV_UnlinkEdict()` in the C codebase.
    pub fn unlink_entity(&mut self, ent_idx: usize) {
        if ent_idx >= self.links.len() {
            return;
        }
        let info = match self.links[ent_idx].take() {
            Some(v) => v,
            None => return, // not linked
        };

        let list = if info.is_trigger {
            &mut self.area_nodes[info.node_idx].trigger_edicts
        } else {
            &mut self.area_nodes[info.node_idx].solid_edicts
        };
        list.retain(|&id| id != ent_idx);
    }

    /// Query all entities whose AABB overlaps the given region.
    ///
    /// Corresponds to `SV_AreaEdicts()` in the C codebase. `area_type`
    /// should be one of `AREA_SOLID` or `AREA_TRIGGERS`.
    pub fn area_edicts(&self, mins: Vec3f, maxs: Vec3f, area_type: i32) -> Vec<usize> {
        let mut result = Vec::new();
        if !self.area_nodes.is_empty() {
            self.area_edicts_r(0, mins, maxs, area_type, &mut result);
        }
        result
    }

    // -- internal: recursive query ------------------------------------------

    fn area_edicts_r(
        &self,
        node_idx: usize,
        mins: Vec3f,
        maxs: Vec3f,
        area_type: i32,
        result: &mut Vec<usize>,
    ) {
        let node = &self.area_nodes[node_idx];

        // Choose the correct list based on area_type.
        let list = if area_type == AREA_SOLID {
            &node.solid_edicts
        } else {
            &node.trigger_edicts
        };

        for &ent_idx in list {
            // Check AABB overlap using the stored link bounds.
            if let Some(link) = &self.links[ent_idx] {
                if link.mins.x > maxs.x
                    || link.mins.y > maxs.y
                    || link.mins.z > maxs.z
                    || link.maxs.x < mins.x
                    || link.maxs.y < mins.y
                    || link.maxs.z < mins.z
                {
                    continue; // not touching
                }
                result.push(ent_idx);
            }
        }

        // Recurse if this is an internal node.
        if node.axis == -1 {
            return;
        }

        let axis = node.axis as usize;
        if Self::component(maxs, axis) > node.dist {
            self.area_edicts_r(node.children[0], mins, maxs, area_type, result);
        }
        if Self::component(mins, axis) < node.dist {
            self.area_edicts_r(node.children[1], mins, maxs, area_type, result);
        }
    }

    // -- helpers ------------------------------------------------------------

    #[inline]
    fn component(v: Vec3f, axis: usize) -> f32 {
        match axis {
            0 => v.x,
            1 => v.y,
            _ => v.z,
        }
    }

    #[inline]
    fn set_component(v: &mut Vec3f, axis: usize, val: f32) {
        match axis {
            0 => v.x = val,
            1 => v.y = val,
            _ => v.z = val,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_world() -> ServerWorld {
        let mut w = ServerWorld::new();
        w.clear(
            Vec3f::new(-4096.0, -4096.0, -4096.0),
            Vec3f::new(4096.0, 4096.0, 4096.0),
        );
        w
    }

    #[test]
    fn world_link_unlink() {
        let mut w = make_world();

        // Link entity 1 as a solid bbox at the origin.
        let mins = Vec3f::new(-16.0, -16.0, -24.0);
        let maxs = Vec3f::new(16.0, 16.0, 32.0);
        w.link_entity(1, mins, maxs, Solid::Bbox);

        // Should find it in a query that covers the origin.
        let found = w.area_edicts(
            Vec3f::new(-100.0, -100.0, -100.0),
            Vec3f::new(100.0, 100.0, 100.0),
            AREA_SOLID,
        );
        assert!(found.contains(&1), "entity 1 should be found after linking");

        // Unlink and verify it's gone.
        w.unlink_entity(1);
        let found = w.area_edicts(
            Vec3f::new(-100.0, -100.0, -100.0),
            Vec3f::new(100.0, 100.0, 100.0),
            AREA_SOLID,
        );
        assert!(
            !found.contains(&1),
            "entity 1 should NOT be found after unlinking"
        );
    }

    #[test]
    fn trigger_vs_solid() {
        let mut w = make_world();
        let mins = Vec3f::new(-10.0, -10.0, -10.0);
        let maxs = Vec3f::new(10.0, 10.0, 10.0);

        w.link_entity(1, mins, maxs, Solid::Bbox); // solid
        w.link_entity(2, mins, maxs, Solid::Trigger); // trigger

        let query_mins = Vec3f::new(-100.0, -100.0, -100.0);
        let query_maxs = Vec3f::new(100.0, 100.0, 100.0);

        let solids = w.area_edicts(query_mins, query_maxs, AREA_SOLID);
        let triggers = w.area_edicts(query_mins, query_maxs, AREA_TRIGGERS);

        assert!(solids.contains(&1));
        assert!(!solids.contains(&2));
        assert!(triggers.contains(&2));
        assert!(!triggers.contains(&1));
    }

    #[test]
    fn area_edicts_respects_bounds() {
        let mut w = make_world();

        // Entity at (1000, 1000, 0).
        w.link_entity(
            1,
            Vec3f::new(990.0, 990.0, -10.0),
            Vec3f::new(1010.0, 1010.0, 10.0),
            Solid::Bbox,
        );

        // Query far away — should not find it.
        let found = w.area_edicts(
            Vec3f::new(-100.0, -100.0, -100.0),
            Vec3f::new(100.0, 100.0, 100.0),
            AREA_SOLID,
        );
        assert!(!found.contains(&1));

        // Query that overlaps — should find it.
        let found = w.area_edicts(
            Vec3f::new(900.0, 900.0, -50.0),
            Vec3f::new(1100.0, 1100.0, 50.0),
            AREA_SOLID,
        );
        assert!(found.contains(&1));
    }

    #[test]
    fn solid_not_entities_are_not_linked() {
        let mut w = make_world();
        let mins = Vec3f::new(-10.0, -10.0, -10.0);
        let maxs = Vec3f::new(10.0, 10.0, 10.0);

        w.link_entity(1, mins, maxs, Solid::Not);

        let found = w.area_edicts(
            Vec3f::new(-100.0, -100.0, -100.0),
            Vec3f::new(100.0, 100.0, 100.0),
            AREA_SOLID,
        );
        assert!(!found.contains(&1));
    }

    #[test]
    fn double_link_updates_position() {
        let mut w = make_world();

        // Link at origin.
        w.link_entity(
            1,
            Vec3f::new(-10.0, -10.0, -10.0),
            Vec3f::new(10.0, 10.0, 10.0),
            Solid::Bbox,
        );

        // Re-link far away. The old entry should be removed automatically.
        w.link_entity(
            1,
            Vec3f::new(3000.0, 3000.0, -10.0),
            Vec3f::new(3020.0, 3020.0, 10.0),
            Solid::Bbox,
        );

        // Should NOT be at the origin any more.
        let found = w.area_edicts(
            Vec3f::new(-100.0, -100.0, -100.0),
            Vec3f::new(100.0, 100.0, 100.0),
            AREA_SOLID,
        );
        assert!(!found.contains(&1));

        // Should be at the new position.
        let found = w.area_edicts(
            Vec3f::new(2900.0, 2900.0, -50.0),
            Vec3f::new(3100.0, 3100.0, 50.0),
            AREA_SOLID,
        );
        assert!(found.contains(&1));
    }
}
