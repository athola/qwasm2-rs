//! Server-side GameImport implementation.
//!
//! `ServerGameImport` implements `q2_game::traits::GameImport` so the game
//! module can call back into the server during a game frame.
//!
//! `GameImport` methods take `&self`, so the server-side state the game
//! reaches (message buffer, configstring mirror, collision map, entity links)
//! lives in a `RefCell`. The engine is single-threaded, so the trait carries
//! no `Send + Sync` bound and no lock is needed. Every method takes and drops
//! its borrow inside the call, because the game calls back in while it runs.

use std::cell::RefCell;

use q2_common::collision::CollisionMap;
use q2_common::error::Q2Result;
use q2_common::net_msg::NetMsg;
use q2_game::traits::{EntityLink, GameImport, ModelInfo};
use q2_shared::{
    constants::{
        CONTENTS_DEADMONSTER, CS_IMAGES, CS_MODELS, CS_SOUNDS, MAX_CONFIGSTRINGS, MAX_EDICTS,
        MAX_IMAGES, MAX_MODELS, MAX_SOUNDS,
    },
    types::*,
};

use crate::vis::{entity_clusters, fat_pvs, in_pvs, EntityClusters, MAX_TOTAL_ENT_LEAFS};
use crate::world::{ServerWorld, AREA_SOLID};

// ---------------------------------------------------------------------------
// Interior-mutable state
// ---------------------------------------------------------------------------

struct GiInner {
    /// Outbound message buffer accumulating write_* calls; discarded (not sent)
    /// when multicast/unicast is called until proper routing is implemented.
    /// Uses `NetMsg` so game-written temp entities encode byte-for-byte like
    /// every other server message (MSG_Write* in C).
    msg_buf: NetMsg,
    /// Game-written configstrings; `Server::frame` copies dirty slots into
    /// `Server::configstrings` after each game frame.
    configstrings: Vec<String>,
    /// Tracks which configstring slots were written since the last drain.
    dirty: Vec<bool>,
    /// The loaded map (C `CM_LoadMap` state).
    collision: CollisionMap,
    /// Area tree for entity queries (C `sv_areanodes`).
    world: ServerWorld,
    /// Last link per entity number: what SV_ClipMoveToEntities and
    /// SV_PointContents read from the edict in C. C reads the live edict,
    /// so a game change to `solid`, `owner`, `svflags`, origin or bounds
    /// takes effect at once; here it takes effect at the next
    /// `link_entity`. Game code that changes those fields must relink, as
    /// C game code does to move the entity in the area tree anyway.
    links: Vec<Option<EntityLink>>,
    /// Visibility clusters per entity number from its last link; kept when
    /// it unlinks, as C leaves `clusternums` in the edict.
    clusters: Vec<EntityClusters>,
    /// C `areanum`/`areanum2` per entity number from its last link (0 for
    /// none); kept when it unlinks, like `clusters`.
    areas: Vec<(i32, i32)>,
    /// Console text the game queued (C `gi.AddCommandString` ->
    /// `Cbuf_AddText`), for the server loop to run.
    commands: String,
    /// Cvars set for the game to read (C's shared cvar table).
    cvars: std::collections::HashMap<String, String>,
    /// Sounds the game started since the last `take_sounds`.
    sounds: Vec<SoundEvent>,
    /// The client command being run (C `Cmd_TokenizeString`'s `cmd_argv`
    /// and `cmd_args`), for the game's `argv`/`args`.
    command_argv: Vec<String>,
    command_args: String,
    /// Messages the game multicast since the last `take_messages`
    /// (muzzle flashes, temp entities), whole.
    messages: Vec<Vec<u8>>,
}

/// One sound the game started: what C `SV_StartSound` writes as an
/// `svc_sound` message. The PHS and reliable routing of that message is a
/// network concern; the one local client hears every sound.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SoundEvent {
    /// Entity number the sound plays on (for channel overrides).
    pub ent: usize,
    /// `CHAN_*` with the routing bits stripped, as the client reads it.
    pub channel: i32,
    /// `CS_SOUNDS` index.
    pub sound_index: i32,
    pub volume: f32,
    pub attenuation: f32,
    /// Delay in seconds, 0..=0.255.
    pub time_ofs: f32,
    /// Where the sound stays (C `SND_POS`), or `None` to follow the
    /// entity as the client sees it move.
    pub origin: Option<Vec3f>,
}

/// Whether `cluster`'s bit is set in a PVS or PHS row; no cluster is not.
fn cluster_bit(row: &[u8], cluster: i32) -> bool {
    usize::try_from(cluster).is_ok_and(|cluster| {
        row.get(cluster >> 3)
            .is_some_and(|bits| bits & (1 << (cluster & 7)) != 0)
    })
}

impl GiInner {
    fn new() -> Self {
        Self {
            msg_buf: NetMsg::new(),
            configstrings: vec![String::new(); MAX_CONFIGSTRINGS],
            dirty: vec![false; MAX_CONFIGSTRINGS],
            collision: CollisionMap::new(),
            world: ServerWorld::new(),
            links: vec![None; MAX_EDICTS],
            clusters: vec![EntityClusters::List(Vec::new()); MAX_EDICTS],
            areas: vec![(0, 0); MAX_EDICTS],
            commands: String::new(),
            cvars: std::collections::HashMap::new(),
            sounds: Vec::new(),
            command_argv: Vec::new(),
            command_args: String::new(),
            messages: Vec::new(),
        }
    }

    /// C SV_StartSound. Out-of-range volume, attenuation or offset is
    /// ERR_FATAL in C; the trait cannot fail, so the sound is logged and
    /// dropped. An explicit `origin` is kept; otherwise a brush model plays
    /// from its center and an entity clients are not sent from its origin
    /// (both sent with `SND_POS`, the client cannot place them), and any
    /// other entity is followed by the client. The origin and flags are
    /// read from the entity's last link; an unlinked entity is followed.
    #[allow(clippy::too_many_arguments)]
    fn start_sound(
        &mut self,
        origin: Option<Vec3f>,
        ent: usize,
        channel: i32,
        sound_index: i32,
        volume: f32,
        attenuation: f32,
        time_ofs: f32,
    ) {
        if !(0.0..=1.0).contains(&volume)
            || !(0.0..=4.0).contains(&attenuation)
            || !(0.0..=0.255).contains(&time_ofs)
        {
            tracing::error!(
                volume,
                attenuation,
                time_ofs,
                "SV_StartSound: bad parameter (C ERR_FATAL)"
            );
            return;
        }
        let origin = origin.or_else(|| {
            let link = self.links.get(ent).copied().flatten()?;
            if link.solid == Solid::Bsp {
                Some(link.origin + (link.mins + link.maxs) * 0.5)
            } else if link.svflags & q2_shared::constants::SVF_NOCLIENT != 0 {
                Some(link.origin)
            } else {
                None
            }
        });
        self.sounds.push(SoundEvent {
            ent,
            channel: channel & 7,
            sound_index,
            volume,
            attenuation,
            time_ofs,
            origin,
        });
    }

    fn set_configstring(&mut self, idx: usize, value: &str) {
        if idx < self.configstrings.len() {
            self.configstrings[idx] = value.to_string();
            self.dirty[idx] = true;
        }
    }

    /// C SV_FindIndex with `create`: the slot already holding `name`, or the
    /// first empty one from 1. A full table is ERR_DROP in C; the trait can't
    /// return an error, so it is logged and 0 (no model/sound) returned.
    fn find_index(&mut self, name: &str, start: usize, max: usize) -> i32 {
        if name.is_empty() {
            return 0;
        }
        let mut i = 1;
        while i < max && !self.configstrings[start + i].is_empty() {
            if self.configstrings[start + i] == name {
                return i as i32;
            }
            i += 1;
        }
        if i == max {
            tracing::error!(name, start, "index overflow (C ERR_DROP)");
            return 0;
        }
        self.set_configstring(start + i, name);
        i as i32
    }

    /// C SV_HullForEntity. Model index 1 is the world (`sv.models[1]`), so
    /// index `n` is inline model `n - 1`.
    fn hull_for_entity(&mut self, link: &EntityLink) -> Option<i32> {
        if link.solid == Solid::Bsp {
            let model = usize::try_from(link.modelindex - 1)
                .ok()
                .and_then(|i| self.collision.model(i));
            if model.is_none() {
                tracing::error!(number = link.number, "MOVETYPE_PUSH with a non bsp model");
            }
            return model.map(|m| m.head_node);
        }
        Some(self.collision.headnode_for_box(link.mins, link.maxs))
    }
}

// ---------------------------------------------------------------------------
// ServerGameImport
// ---------------------------------------------------------------------------

/// Server implementation of the `GameImport` trait.
pub struct ServerGameImport {
    inner: RefCell<GiInner>,
}

impl std::fmt::Debug for ServerGameImport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerGameImport").finish_non_exhaustive()
    }
}

impl ServerGameImport {
    pub fn new() -> Self {
        Self {
            inner: RefCell::new(GiInner::new()),
        }
    }

    /// Tokenize a client's command for the game to read (C
    /// `SV_ExecuteUserCommand` -> `Cmd_TokenizeString`): `args` is the
    /// raw rest of the line after the first token, trailing whitespace
    /// cut.
    pub fn set_client_command(&self, text: &str) {
        let line = text.split('\n').next().unwrap_or_default();
        let rest = line.trim_start_matches(|c: char| c <= ' ');
        let rest = rest.trim_start_matches(|c: char| c > ' ');
        let args = rest
            .trim_start_matches(|c: char| c <= ' ')
            .trim_end_matches(|c: char| c <= ' ');
        let mut inner = self.inner.borrow_mut();
        inner.command_argv = q2_common::cmd::tokenize(text);
        inner.command_args = args.to_string();
    }

    /// Set cvar `name` for the game (C `Cvar_Set` on the shared table).
    pub fn set_cvar(&self, name: &str, value: &str) {
        self.inner
            .borrow_mut()
            .cvars
            .insert(name.to_string(), value.to_string());
    }

    /// The console text queued by `add_command_string` since the last call.
    pub fn take_commands(&self) -> String {
        std::mem::take(&mut self.inner.borrow_mut().commands)
    }

    /// Queue an `svc_print` of `msg` at `level` for the local client.
    fn queue_print(&self, level: i32, msg: &str) {
        let mut message = NetMsg::new();
        message.write_byte(q2_shared::protocol::SvcOp::Print as i32);
        message.write_byte(level);
        message.write_string(msg);
        self.inner
            .borrow_mut()
            .messages
            .push(message.data().to_vec());
    }

    /// The messages the game multicast since the last call, in order.
    pub fn take_messages(&self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.inner.borrow_mut().messages)
    }

    /// The sounds the game started since the last call, in order.
    pub fn take_sounds(&self) -> Vec<SoundEvent> {
        std::mem::take(&mut self.inner.borrow_mut().sounds)
    }

    /// Load a map's collision data and rebuild the area tree (C CM_LoadMap +
    /// SV_ClearWorld). Links and the configstring mirror start empty, as C
    /// SV_SpawnServer zeroes `sv`. Returns the map checksum.
    pub fn load_map(&self, bsp: &[u8]) -> Q2Result<u32> {
        let mut inner = self.inner.borrow_mut();
        let checksum = inner.collision.load_map(bsp)?;
        let (mins, maxs) = inner
            .collision
            .model(0)
            .map(|m| (m.mins, m.maxs))
            .unwrap_or_default();
        inner.world.clear(mins, maxs);
        inner.links.fill(None);
        inner.clusters.fill(EntityClusters::List(Vec::new()));
        inner.areas.fill((0, 0));
        inner.configstrings.fill(String::new());
        inner.dirty.fill(false);
        Ok(checksum)
    }

    /// C `SV_FatPVS`: what can be seen from the eye at `org`, as one bit
    /// per cluster.
    pub fn fat_pvs(&self, org: Vec3f) -> Vec<u8> {
        let inner = self.inner.borrow();
        let reach = Vec3f::new(8.0, 8.0, 8.0);
        let (leafs, _) = inner.collision.box_leafnums(org - reach, org + reach, 64);
        let clusters: Vec<i32> = leafs
            .iter()
            .map(|&leaf| i32::from(inner.collision.leaf_cluster(leaf)))
            .collect();
        fat_pvs(&clusters, |cluster| inner.collision.cluster_pvs(cluster))
    }

    /// Whether entity `number`, where it was last linked, touches a cluster
    /// set in `pvs` (the PVS test of C `SV_BuildClientFrame`).
    pub fn entity_in_pvs(&self, number: usize, pvs: &[u8]) -> bool {
        let inner = self.inner.borrow();
        inner.clusters.get(number).is_some_and(|clusters| {
            in_pvs(clusters, pvs, |node, bits| {
                inner.collision.headnode_visible(node, bits)
            })
        })
    }

    /// Entity `number`'s C `areanum` and `areanum2` (0 for none) from its
    /// last link.
    pub fn entity_areas(&self, number: usize) -> (i32, i32) {
        self.inner
            .borrow()
            .areas
            .get(number)
            .copied()
            .unwrap_or((0, 0))
    }

    /// Whether the areas of two leafs are connected (the area check of C
    /// PF_inPVS/PF_inPHS).
    fn leaf_areas_connected(&self, leaf1: usize, leaf2: usize) -> bool {
        let (area1, area2) = {
            let inner = self.inner.borrow();
            let cm = &inner.collision;
            (
                i32::from(cm.leaf_area(leaf1)),
                i32::from(cm.leaf_area(leaf2)),
            )
        };
        self.areas_connected(area1, area2)
    }

    /// Which areas a client in `area` can see (C `SV_BuildClientFrame`'s
    /// `CM_WriteAreaBits`), or `None`, which draws every area, when `area`
    /// is not in the map.
    pub fn area_bits(&self, area: i32) -> Option<Vec<u8>> {
        let bits = self.inner.borrow().collision.write_area_bits(area, false);
        bits.map_err(|e| tracing::warn!(area, "area_bits: {e}"))
            .ok()
    }

    /// The area holding `point` (C `CM_LeafArea(CM_PointLeafnum(p))`).
    pub fn point_area(&self, point: Vec3f) -> i32 {
        let inner = self.inner.borrow();
        let cm = &inner.collision;
        i32::from(cm.leaf_area(cm.point_leafnum(point)))
    }

    /// C `SV_BuildClientFrame`'s `clientphs`: the PHS row of the cluster
    /// holding `eye` (the eye's own leaf, not the fat PVS box).
    pub fn client_phs(&self, eye: Vec3f) -> Vec<u8> {
        let inner = self.inner.borrow();
        let cm = &inner.collision;
        cm.cluster_phs(i32::from(cm.leaf_cluster(cm.point_leafnum(eye))))
    }

    /// The beam test of C `SV_BuildClientFrame`: whether the first cluster
    /// entity `number` was linked in is set in `phs`. An entity listed by
    /// headnode asks the tree, as C's first cluster is then one of many.
    pub fn entity_in_phs(&self, number: usize, phs: &[u8]) -> bool {
        let inner = self.inner.borrow();
        match inner.clusters.get(number) {
            Some(EntityClusters::List(list)) => list
                .first()
                .is_some_and(|&cluster| cluster_bit(phs, cluster)),
            Some(EntityClusters::Headnode(node)) => inner.collision.headnode_visible(*node, phs),
            None => false,
        }
    }

    /// The loaded map's entity lump.
    pub fn entity_string(&self) -> String {
        self.inner.borrow().collision.entity_string().to_string()
    }

    /// Inline (brush) models in the loaded map, the world included.
    pub fn num_inline_models(&self) -> usize {
        self.inner.borrow().collision.num_inline_models()
    }

    /// Drain configstring updates written since the last call.
    ///
    /// Returns `(index, value)` pairs for every slot that was written via
    /// `configstring()`, including slots cleared to an empty string (valid Q2
    /// protocol to erase a slot).  Clears the dirty flags so subsequent calls
    /// return only new writes.
    pub fn drain_configstring_updates(&self) -> Vec<(usize, String)> {
        let mut inner = self.inner.borrow_mut();
        let updates: Vec<(usize, String)> = inner
            .dirty
            .iter()
            .enumerate()
            .filter_map(|(i, &d)| {
                if d {
                    Some((i, inner.configstrings[i].clone()))
                } else {
                    None
                }
            })
            .collect();
        for (i, _) in &updates {
            inner.dirty[*i] = false;
        }
        updates
    }
}

impl Default for ServerGameImport {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// GameImport impl
// ---------------------------------------------------------------------------

impl GameImport for ServerGameImport {
    // -- printing ------------------------------------------------------------

    /// C SV_BroadcastPrintf: every client gets an `svc_print`.
    fn bprintf(&self, printlevel: i32, msg: &str) {
        tracing::info!("[bprintf] {}", msg.trim_end());
        self.queue_print(printlevel, msg);
    }

    fn dprintf(&self, msg: &str) {
        tracing::debug!("[dprintf] {}", msg.trim_end());
    }

    /// C PF_cprintf: to a client as `svc_print`, or the console for none.
    fn cprintf(&self, ent_idx: Option<usize>, printlevel: i32, msg: &str) {
        tracing::debug!(ent = ?ent_idx, "[cprintf] {}", msg.trim_end());
        if ent_idx.is_some() {
            self.queue_print(printlevel, msg);
        }
    }

    /// C PF_centerprintf: an `svc_centerprint` to the client.
    fn centerprintf(&self, ent_idx: Option<usize>, msg: &str) {
        tracing::debug!(ent = ?ent_idx, "[centerprintf] {}", msg.trim_end());
        let mut message = NetMsg::new();
        message.write_byte(q2_shared::protocol::SvcOp::CenterPrint as i32);
        message.write_string(msg);
        self.inner
            .borrow_mut()
            .messages
            .push(message.data().to_vec());
    }

    // -- sound ---------------------------------------------------------------

    /// C PF_StartSound: no entity, no sound.
    fn sound(
        &self,
        ent_idx: Option<usize>,
        channel: i32,
        sound_index: i32,
        volume: f32,
        attenuation: f32,
        time_ofs: f32,
    ) {
        if let Some(ent) = ent_idx {
            self.inner.borrow_mut().start_sound(
                None,
                ent,
                channel,
                sound_index,
                volume,
                attenuation,
                time_ofs,
            );
        }
    }

    fn positioned_sound(
        &self,
        origin: Vec3f,
        ent_idx: usize,
        channel: i32,
        sound_index: i32,
        volume: f32,
        attenuation: f32,
        time_ofs: f32,
    ) {
        self.inner.borrow_mut().start_sound(
            Some(origin),
            ent_idx,
            channel,
            sound_index,
            volume,
            attenuation,
            time_ofs,
        );
    }

    // -- asset indexing ------------------------------------------------------

    fn model_index(&self, name: &str) -> i32 {
        self.inner
            .borrow_mut()
            .find_index(name, CS_MODELS, MAX_MODELS)
    }

    fn sound_index(&self, name: &str) -> i32 {
        self.inner
            .borrow_mut()
            .find_index(name, CS_SOUNDS, MAX_SOUNDS)
    }

    fn image_index(&self, name: &str) -> i32 {
        self.inner
            .borrow_mut()
            .find_index(name, CS_IMAGES, MAX_IMAGES)
    }

    /// C PF_setmodel: SV_ModelIndex (`*n` is already registered by
    /// SV_SpawnServer) plus CM_InlineModel's bounds for `*n`. A bad `*n`
    /// (including `*0`) is ERR_DROP in C; the trait cannot fail, so it is
    /// logged and the entity gets no bounds.
    fn set_model(&self, _ent_idx: usize, name: &str) -> ModelInfo {
        let index = self.model_index(name);
        let bounds = name
            .strip_prefix('*')
            .and_then(|n| n.parse::<usize>().ok())
            // C CM_InlineModel: `*0` is the world, not an entity's model.
            .filter(|&n| n >= 1)
            .and_then(|n| {
                let inner = self.inner.borrow();
                inner.collision.model(n).map(|m| (m.mins, m.maxs))
            });
        if name.starts_with('*') && bounds.is_none() {
            tracing::error!(name, "CM_InlineModel: bad number");
        }
        ModelInfo { index, bounds }
    }

    // -- collision -----------------------------------------------------------

    /// C SV_Trace: the world, then every linked solid entity in the move's
    /// bounds (SV_ClipMoveToEntities). `ent_index` is the world (0) unless an
    /// entity was hit.
    fn trace(
        &self,
        start: Vec3f,
        mins: Vec3f,
        maxs: Vec3f,
        end: Vec3f,
        pass_ent: Option<usize>,
        content_mask: i32,
    ) -> Trace {
        let mut guard = self.inner.borrow_mut();
        let inner = &mut *guard;
        let mut trace = inner
            .collision
            .box_trace(start, end, mins, maxs, 0, content_mask);
        trace.ent_index = Some(0);
        if trace.fraction == 0.0 {
            return trace; // blocked by the world
        }

        // SV_TraceBounds.
        let (mut boxmins, mut boxmaxs) = (Vec3f::ZERO, Vec3f::ZERO);
        for i in 0..3 {
            let (lo, hi) = if end[i] > start[i] {
                (start[i], end[i])
            } else {
                (end[i], start[i])
            };
            boxmins[i] = lo + mins[i] - 1.0;
            boxmaxs[i] = hi + maxs[i] + 1.0;
        }

        let pass_owner = pass_ent
            .and_then(|p| inner.links.get(p).copied().flatten())
            .and_then(|l| l.owner);
        for number in inner.world.area_edicts(boxmins, boxmaxs, AREA_SOLID) {
            let Some(touch) = inner.links[number] else {
                continue;
            };
            if touch.solid == Solid::Not || Some(number) == pass_ent {
                continue;
            }
            if trace.allsolid {
                return trace;
            }
            if pass_ent.is_some() && (touch.owner == pass_ent || pass_owner == Some(number)) {
                continue; // own missiles, or the owner
            }
            if content_mask & CONTENTS_DEADMONSTER == 0
                && touch.svflags & q2_shared::constants::SVF_DEADMONSTER != 0
            {
                continue;
            }
            let Some(head_node) = inner.hull_for_entity(&touch) else {
                continue;
            };
            let angles = if touch.solid == Solid::Bsp {
                touch.angles
            } else {
                Vec3f::ZERO // boxes don't rotate
            };
            // C passes mins2/maxs2 for SVF_MONSTER entities; SV_Trace copies
            // them from mins/maxs, so both branches trace the same box.
            let mut hit = inner.collision.transformed_box_trace(
                start,
                end,
                mins,
                maxs,
                head_node,
                content_mask,
                touch.origin,
                angles,
            );
            if hit.allsolid || hit.startsolid || hit.fraction < trace.fraction {
                hit.ent_index = Some(number);
                let was_startsolid = trace.startsolid;
                trace = hit;
                if was_startsolid {
                    trace.startsolid = true;
                }
            }
        }
        trace
    }

    /// C SV_PointContents: the world's contents ORed with every linked solid
    /// entity at the point.
    fn point_contents(&self, point: Vec3f) -> i32 {
        let mut guard = self.inner.borrow_mut();
        let inner = &mut *guard;
        let mut contents = inner.collision.point_contents(point, 0);
        for number in inner.world.area_edicts(point, point, AREA_SOLID) {
            let Some(link) = inner.links[number] else {
                continue;
            };
            if let Some(head_node) = inner.hull_for_entity(&link) {
                contents |= inner.collision.transformed_point_contents(
                    point,
                    head_node,
                    link.origin,
                    link.angles,
                );
            }
        }
        contents
    }

    /// C PF_inPVS: whether `p2`'s cluster is in the PVS of `p1`'s and no
    /// closed door (area portal) parts them. A point in no cluster (solid,
    /// outside the map) is not visible; a map without vis data sees
    /// everything.
    fn in_pvs(&self, p1: Vec3f, p2: Vec3f) -> bool {
        let (leaf1, leaf2, visible) = {
            let inner = self.inner.borrow();
            let cm = &inner.collision;
            let (leaf1, leaf2) = (cm.point_leafnum(p1), cm.point_leafnum(p2));
            let mask = cm.cluster_pvs(i32::from(cm.leaf_cluster(leaf1)));
            let cluster = i32::from(cm.leaf_cluster(leaf2));
            (leaf1, leaf2, cluster_bit(&mask, cluster))
        };
        visible && self.leaf_areas_connected(leaf1, leaf2)
    }

    /// C PF_inPHS: `in_pvs` against the PHS row of `p1`'s cluster.
    fn in_phs(&self, p1: Vec3f, p2: Vec3f) -> bool {
        let (leaf1, leaf2, audible) = {
            let inner = self.inner.borrow();
            let cm = &inner.collision;
            let (leaf1, leaf2) = (cm.point_leafnum(p1), cm.point_leafnum(p2));
            let mask = cm.cluster_phs(i32::from(cm.leaf_cluster(leaf1)));
            let cluster = i32::from(cm.leaf_cluster(leaf2));
            (leaf1, leaf2, cluster_bit(&mask, cluster))
        };
        audible && self.leaf_areas_connected(leaf1, leaf2)
    }

    // -- area portals --------------------------------------------------------

    /// C CM_SetAreaPortalState raises ERR_DROP for a portal number past
    /// the map's; the trait cannot fail, so the change is logged and
    /// dropped, as `configstring` does.
    fn set_area_portal_state(&self, portal_num: i32, open: bool) {
        let result = usize::try_from(portal_num)
            .map_err(|_| q2_common::error::Q2Error::Drop("areaportal < 0".into()))
            .and_then(|n| {
                self.inner
                    .borrow_mut()
                    .collision
                    .set_area_portal_state(n, open)
            });
        if let Err(e) = result {
            tracing::warn!(portal_num, "set_area_portal_state: {e}");
        }
    }

    /// C CM_AreasConnected; an area past the map's (ERR_DROP in C) is
    /// logged and treated as cut off.
    fn areas_connected(&self, area1: i32, area2: i32) -> bool {
        let connected = self.inner.borrow().collision.areas_connected(area1, area2);
        connected.unwrap_or_else(|e| {
            tracing::warn!(area1, area2, "areas_connected: {e}");
            false
        })
    }

    fn entity_areas(&self, number: usize) -> (i32, i32) {
        ServerGameImport::entity_areas(self, number)
    }

    // -- entity linking ------------------------------------------------------

    fn link_entity(&self, ent: &EntityLink) {
        if ent.number >= MAX_EDICTS {
            tracing::warn!(number = ent.number, "link_entity: number beyond MAX_EDICTS");
            return;
        }
        let mut inner = self.inner.borrow_mut();
        inner
            .world
            .link_entity(ent.number, ent.absmin, ent.absmax, ent.solid);
        inner.links[ent.number] = Some(*ent);
        // C SV_LinkEdict: link to the PVS leafs the box touches.
        let (leafs, topnode) =
            inner
                .collision
                .box_leafnums(ent.absmin, ent.absmax, MAX_TOTAL_ENT_LEAFS);
        let leaf_clusters: Vec<i32> = leafs
            .iter()
            .map(|&leaf| i32::from(inner.collision.leaf_cluster(leaf)))
            .collect();
        inner.clusters[ent.number] = entity_clusters(&leaf_clusters, topnode);
        // Doors may legally straddle two areas, but nothing should ever
        // need more than that.
        let (mut area1, mut area2) = (0, 0);
        for &leaf in &leafs {
            let area = i32::from(inner.collision.leaf_area(leaf));
            if area == 0 {
                continue;
            }
            if area1 != 0 && area1 != area {
                if area2 != 0 && area2 != area {
                    tracing::debug!(absmin = ?ent.absmin, "object touching 3 areas");
                }
                area2 = area;
            } else {
                area1 = area;
            }
        }
        inner.areas[ent.number] = (area1, area2);
    }

    fn unlink_entity(&self, number: usize) {
        if number >= MAX_EDICTS {
            return;
        }
        let mut inner = self.inner.borrow_mut();
        inner.world.unlink_entity(number);
        inner.links[number] = None;
    }

    fn box_edicts(&self, mins: Vec3f, maxs: Vec3f, max_count: usize, area_type: i32) -> Vec<usize> {
        let mut found = self.inner.borrow().world.area_edicts(mins, maxs, area_type);
        found.truncate(max_count);
        found
    }

    // -- configstrings -------------------------------------------------------

    /// C PF_Configstring raises ERR_DROP for an index outside
    /// `0..MAX_CONFIGSTRINGS`; the trait cannot fail, so the write is
    /// logged and dropped.
    fn configstring(&self, num: i32, string: &str) {
        let Some(index) = usize::try_from(num).ok().filter(|&i| i < MAX_CONFIGSTRINGS) else {
            tracing::error!(num, "configstring: bad index (C ERR_DROP)");
            return;
        };
        self.inner.borrow_mut().set_configstring(index, string);
    }

    // -- network writing (buffered; flushed on multicast/unicast) -----------

    fn write_byte(&self, c: i32) {
        self.inner.borrow_mut().msg_buf.write_byte(c);
    }

    fn write_short(&self, c: i32) {
        self.inner.borrow_mut().msg_buf.write_short(c);
    }

    fn write_long(&self, c: i32) {
        self.inner.borrow_mut().msg_buf.write_long(c);
    }

    fn write_float(&self, f: f32) {
        self.inner.borrow_mut().msg_buf.write_float(f);
    }

    fn write_string(&self, s: &str) {
        self.inner.borrow_mut().msg_buf.write_string(s);
    }

    fn write_position(&self, pos: Vec3f) {
        self.inner.borrow_mut().msg_buf.write_position(pos);
    }

    fn write_dir(&self, dir: Vec3f) {
        self.inner.borrow_mut().msg_buf.write_dir(dir);
    }

    fn write_angle(&self, f: f32) {
        self.inner.borrow_mut().msg_buf.write_angle(f);
    }

    // -- multicast / unicast (flush message buffer) -------------------------

    /// C SV_Multicast. The one local client gets every message; the
    /// PVS/PHS choice and reliable routing only pick network clients.
    fn multicast(&self, _origin: Vec3f, _to: Multicast) {
        let mut inner = self.inner.borrow_mut();
        if !inner.msg_buf.is_empty() {
            let message = inner.msg_buf.data().to_vec();
            inner.messages.push(message);
        }
        inner.msg_buf.clear();
    }

    /// C PF_Unicast: the one local client gets the message.
    fn unicast(&self, _ent_idx: usize, _reliable: bool) {
        let mut inner = self.inner.borrow_mut();
        if !inner.msg_buf.is_empty() {
            let message = inner.msg_buf.data().to_vec();
            inner.messages.push(message);
        }
        inner.msg_buf.clear();
    }

    // -- command args --------------------------------------------------------

    fn argc(&self) -> i32 {
        self.inner.borrow().command_argv.len() as i32
    }

    fn argv(&self, n: i32) -> String {
        let inner = self.inner.borrow();
        usize::try_from(n)
            .ok()
            .and_then(|n| inner.command_argv.get(n).cloned())
            .unwrap_or_default()
    }

    fn args(&self) -> String {
        self.inner.borrow().command_args.clone()
    }

    fn add_command_string(&self, text: &str) {
        self.inner.borrow_mut().commands.push_str(text);
    }

    fn cvar(&self, name: &str, default: &str) -> String {
        self.inner
            .borrow()
            .cvars
            .get(name)
            .cloned()
            .unwrap_or_else(|| default.to_string())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn link(gi: &ServerGameImport, number: usize, solid: Solid, svflags: u32) {
        gi.link_entity(&EntityLink {
            number,
            origin: Vec3f::new(100.0, 0.0, 0.0),
            mins: Vec3f::new(-10.0, -20.0, 0.0),
            maxs: Vec3f::new(30.0, 20.0, 40.0),
            solid,
            svflags,
            ..Default::default()
        });
    }

    fn event(ent: usize, channel: i32, origin: Option<Vec3f>) -> SoundEvent {
        SoundEvent {
            ent,
            channel,
            sound_index: 5,
            volume: 0.5,
            attenuation: 1.0,
            time_ofs: 0.1,
            origin,
        }
    }

    #[test]
    fn prints_reach_the_local_client_as_svc_print_and_centerprint() {
        use q2_shared::protocol::SvcOp;
        let gi = ServerGameImport::new();
        gi.bprintf(2, "Ann died.\n");
        gi.cprintf(Some(1), 1, "No cells.\n");
        gi.cprintf(None, 1, "console only\n");
        gi.centerprintf(Some(1), "The door is locked");
        let mut print = vec![SvcOp::Print as u8, 2];
        print.extend_from_slice(b"Ann died.\n\0");
        let mut cprint = vec![SvcOp::Print as u8, 1];
        cprint.extend_from_slice(b"No cells.\n\0");
        let mut center = vec![SvcOp::CenterPrint as u8];
        center.extend_from_slice(b"The door is locked\0");
        assert_eq!(gi.take_messages(), vec![print, cprint, center]);
    }

    #[test]
    fn a_unicast_message_is_queued_for_the_local_client() {
        let gi = ServerGameImport::new();
        gi.write_byte(4);
        gi.write_string("xv 0");
        gi.unicast(1, true);
        assert_eq!(gi.take_messages(), vec![vec![4, b'x', b'v', b' ', b'0', 0]]);
    }

    #[test]
    fn a_multicast_message_is_queued_for_the_local_client() {
        let gi = ServerGameImport::new();
        gi.write_byte(1);
        gi.write_short(5);
        gi.write_byte(7);
        gi.multicast(Vec3f::ZERO, Multicast::PVS);
        gi.write_byte(3);
        gi.multicast(Vec3f::ZERO, Multicast::PHS);
        assert_eq!(gi.take_messages(), vec![vec![1, 5, 0, 7], vec![3]]);
        assert!(gi.take_messages().is_empty(), "taken once");
        assert_eq!(gi.inner.borrow().msg_buf.len(), 0, "the buffer starts over");
    }

    #[test]
    fn an_empty_multicast_queues_nothing() {
        let gi = ServerGameImport::new();
        gi.multicast(Vec3f::ZERO, Multicast::All);
        assert!(gi.take_messages().is_empty());
    }

    #[test]
    fn a_client_command_is_tokenized_for_argv_and_args_like_c() {
        let gi = ServerGameImport::new();
        gi.set_client_command("use  Super Shotgun \n");
        assert_eq!(gi.argc(), 3);
        assert_eq!(gi.argv(0), "use");
        assert_eq!(gi.argv(2), "Shotgun");
        assert_eq!(gi.argv(3), "");
        assert_eq!(gi.argv(-1), "");
        // C Cmd_Args: the raw rest of the line, trailing whitespace cut.
        assert_eq!(gi.args(), "Super Shotgun");
    }

    #[test]
    fn args_keeps_quotes_as_typed() {
        let gi = ServerGameImport::new();
        gi.set_client_command("say \"hi there\"");
        assert_eq!(gi.argv(1), "hi there");
        assert_eq!(gi.args(), "\"hi there\"");
        gi.set_client_command("weapnext");
        assert_eq!((gi.argc(), gi.args()), (1, String::new()));
    }

    #[test]
    fn sound_on_an_entity_follows_the_entity_on_the_client() {
        // C SV_StartSound sends no SND_POS for an ordinary entity.
        let gi = ServerGameImport::new();
        link(&gi, 3, Solid::Bbox, 0);
        gi.sound(Some(3), 2, 5, 0.5, 1.0, 0.1);
        assert_eq!(gi.take_sounds(), vec![event(3, 2, None)]);
        assert_eq!(gi.take_sounds(), vec![]);
    }

    #[test]
    fn sound_on_a_brush_model_is_positioned_at_its_center() {
        let gi = ServerGameImport::new();
        link(&gi, 4, Solid::Bsp, 0);
        gi.sound(Some(4), 2, 5, 0.5, 1.0, 0.1);
        let center = Some(Vec3f::new(110.0, 0.0, 20.0));
        assert_eq!(gi.take_sounds(), vec![event(4, 2, center)]);
    }

    #[test]
    fn sound_on_an_entity_clients_are_not_sent_is_positioned_at_its_origin() {
        let gi = ServerGameImport::new();
        link(&gi, 5, Solid::Not, q2_shared::constants::SVF_NOCLIENT);
        gi.sound(Some(5), 2, 5, 0.5, 1.0, 0.1);
        let origin = Some(Vec3f::new(100.0, 0.0, 0.0));
        assert_eq!(gi.take_sounds(), vec![event(5, 2, origin)]);
    }

    #[test]
    fn sound_on_an_unlinked_entity_follows_the_entity() {
        let gi = ServerGameImport::new();
        gi.sound(Some(6), 2, 5, 0.5, 1.0, 0.1);
        assert_eq!(gi.take_sounds(), vec![event(6, 2, None)]);
    }

    #[test]
    fn sound_without_an_entity_is_dropped() {
        // C PF_StartSound returns on a NULL entity.
        let gi = ServerGameImport::new();
        gi.sound(None, 2, 5, 0.5, 1.0, 0.1);
        assert_eq!(gi.take_sounds(), vec![]);
    }

    #[test]
    fn positioned_sound_keeps_its_origin_even_on_a_brush_model() {
        let gi = ServerGameImport::new();
        link(&gi, 4, Solid::Bsp, 0);
        let at = Vec3f::new(1.0, 2.0, 3.0);
        gi.positioned_sound(at, 4, 2, 5, 0.5, 1.0, 0.1);
        assert_eq!(gi.take_sounds(), vec![event(4, 2, Some(at))]);
    }

    #[test]
    fn sound_channel_keeps_only_the_low_three_bits() {
        // CHAN_NO_PHS_ADD (8) and CHAN_RELIABLE (16) choose how C sends the
        // sound; the client sees `sendchan & 7`.
        let gi = ServerGameImport::new();
        gi.sound(Some(3), 2 | 8 | 16, 5, 0.5, 1.0, 0.1);
        assert_eq!(gi.take_sounds(), vec![event(3, 2, None)]);
    }

    #[test]
    fn sound_with_out_of_range_volume_attenuation_or_offset_is_dropped() {
        // C SV_StartSound: Com_Error(ERR_FATAL) for each.
        let gi = ServerGameImport::new();
        gi.sound(Some(3), 0, 5, -0.1, 1.0, 0.0);
        gi.sound(Some(3), 0, 5, 1.1, 1.0, 0.0);
        gi.sound(Some(3), 0, 5, 1.0, -0.1, 0.0);
        gi.sound(Some(3), 0, 5, 1.0, 4.1, 0.0);
        gi.sound(Some(3), 0, 5, 1.0, 1.0, -0.01);
        gi.sound(Some(3), 0, 5, 1.0, 1.0, 0.256);
        assert_eq!(gi.take_sounds(), vec![]);
        // The limits themselves are allowed.
        gi.sound(Some(3), 0, 5, 1.0, 4.0, 0.255);
        assert_eq!(gi.take_sounds().len(), 1);
    }

    #[test]
    fn the_game_reads_a_set_cvar_and_otherwise_its_default() {
        let gi = ServerGameImport::new();
        assert_eq!(gi.cvar("cl_gun", "2"), "2");
        gi.set_cvar("cl_gun", "0");
        assert_eq!(gi.cvar("cl_gun", "2"), "0");
    }

    #[test]
    fn add_command_string_queues_text_until_the_server_takes_it() {
        let gi = ServerGameImport::new();
        gi.add_command_string("gamemap \"base2\"\n");
        gi.add_command_string("echo hi\n");
        assert_eq!(gi.take_commands(), "gamemap \"base2\"\necho hi\n");
        assert_eq!(gi.take_commands(), "");
    }

    #[test]
    fn unobstructed_trace_ends_at_requested_end_point() {
        let gi = ServerGameImport::new();
        let end = Vec3f::new(100.0, -20.0, 8.0);
        let tr = gi.trace(Vec3f::ZERO, Vec3f::ZERO, Vec3f::ZERO, end, None, 0);
        assert_eq!(tr.fraction, 1.0);
        assert_eq!(tr.endpos, end);
        assert!(!tr.allsolid && !tr.startsolid);
    }

    #[test]
    fn configstring_stored() {
        let gi = ServerGameImport::new();
        gi.configstring(0, "base1");
        gi.configstring(1, "models/world.bsp");

        let updates = gi.drain_configstring_updates();
        let map: std::collections::HashMap<_, _> = updates.into_iter().collect();
        assert_eq!(map[&0], "base1");
        assert_eq!(map[&1], "models/world.bsp");
    }

    #[test]
    fn write_byte_then_unicast_clears_buf() {
        let gi = ServerGameImport::new();
        gi.write_byte(42);
        gi.write_byte(99);
        {
            let inner = gi.inner.borrow();
            assert_eq!(inner.msg_buf.len(), 2);
        }
        gi.unicast(0, true);
        {
            let inner = gi.inner.borrow();
            assert!(inner.msg_buf.is_empty());
        }
    }

    #[test]
    fn write_string_null_terminates() {
        let gi = ServerGameImport::new();
        gi.write_string("hello");
        let inner = gi.inner.borrow();
        assert_eq!(inner.msg_buf.data().last(), Some(&0u8));
    }

    #[test]
    fn configstring_still_writable_after_panic_during_buffer_access() {
        let gi = ServerGameImport::new();
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = gi.inner.borrow_mut();
            panic!("game callback panicked while the buffer was borrowed");
        }));
        assert!(caught.is_err());

        gi.configstring(2, "after_panic");
        let updates = gi.drain_configstring_updates();
        assert_eq!(updates, vec![(2, "after_panic".to_string())]);
    }

    #[test]
    fn write_short_encodes_little_endian() {
        let gi = ServerGameImport::new();
        gi.write_short(0x1234);
        let inner = gi.inner.borrow();
        assert_eq!(inner.msg_buf.data(), &0x1234i16.to_le_bytes());
    }

    #[test]
    fn write_long_encodes_little_endian() {
        let gi = ServerGameImport::new();
        gi.write_long(0x0A0B_0C0D);
        let inner = gi.inner.borrow();
        assert_eq!(inner.msg_buf.data(), &0x0A0B_0C0Di32.to_le_bytes());
    }

    #[test]
    fn write_float_encodes_little_endian() {
        let gi = ServerGameImport::new();
        gi.write_float(1.5);
        let inner = gi.inner.borrow();
        assert_eq!(inner.msg_buf.data(), &1.5f32.to_le_bytes());
    }

    #[test]
    fn write_position_scales_by_8() {
        let gi = ServerGameImport::new();
        gi.write_position(q2_shared::types::Vec3f::new(16.0, -8.0, 0.5));
        let inner = gi.inner.borrow();
        // coord wire format: (int)(f * 8) written as a short, three components
        let x: i16 = i16::from_le_bytes([inner.msg_buf.data()[0], inner.msg_buf.data()[1]]);
        let y: i16 = i16::from_le_bytes([inner.msg_buf.data()[2], inner.msg_buf.data()[3]]);
        let z: i16 = i16::from_le_bytes([inner.msg_buf.data()[4], inner.msg_buf.data()[5]]);
        assert_eq!(x, (16.0f32 * 8.0) as i16);
        assert_eq!(y, (-8.0f32 * 8.0) as i16);
        assert_eq!(z, (0.5f32 * 8.0) as i16);
    }

    fn written(gi: &ServerGameImport) -> Vec<u8> {
        gi.inner.borrow().msg_buf.data().to_vec()
    }

    fn netmsg_bytes(write: impl FnOnce(&mut q2_common::net_msg::NetMsg)) -> Vec<u8> {
        let mut msg = q2_common::net_msg::NetMsg::new();
        write(&mut msg);
        msg.data().to_vec()
    }

    #[test]
    fn write_dir_up_encodes_bytedirs_index_5() {
        let gi = ServerGameImport::new();
        gi.write_dir(Vec3f::new(0.0, 0.0, 1.0));
        assert_eq!(&written(&gi)[..], &[5u8]);
    }

    #[test]
    fn write_dir_matches_netmsg_encoding() {
        let dirs = [
            Vec3f::new(1.0, 0.0, 0.0),
            Vec3f::new(0.0, -1.0, 0.0),
            Vec3f::new(0.0, 0.0, -1.0),
            Vec3f::new(0.6, 0.0, 0.8),
            Vec3f::new(-0.48, 0.36, 0.8),
        ];
        for dir in dirs {
            let gi = ServerGameImport::new();
            gi.write_dir(dir);
            let expected = netmsg_bytes(|m| m.write_dir(dir));
            assert_eq!(&written(&gi)[..], &expected[..], "dir {dir:?}");
        }
    }

    #[test]
    fn write_position_beyond_4096_wraps_like_netmsg() {
        let pos = Vec3f::new(5000.0, -5000.0, 4095.875);
        let gi = ServerGameImport::new();
        gi.write_position(pos);
        let expected = netmsg_bytes(|m| m.write_position(pos));
        assert_eq!(&written(&gi)[..], &expected[..]);
    }

    #[test]
    fn write_angle_quantizes_correctly() {
        let gi = ServerGameImport::new();
        // 90 degrees → (90 * 256 / 360) & 255 = 64
        gi.write_angle(90.0);
        let inner = gi.inner.borrow();
        let expected = ((90.0f32 * 256.0 / 360.0) as i32 & 255) as u8;
        assert_eq!(inner.msg_buf.data()[0], expected);
    }

    #[test]
    fn multicast_clears_buf() {
        let gi = ServerGameImport::new();
        gi.write_byte(1);
        gi.write_byte(2);
        gi.multicast(
            q2_shared::types::Vec3f::new(0.0, 0.0, 0.0),
            q2_shared::types::Multicast::All,
        );
        let inner = gi.inner.borrow();
        assert!(inner.msg_buf.is_empty());
    }

    #[test]
    fn configstring_negative_index_ignored() {
        let gi = ServerGameImport::new();
        gi.configstring(-1, "should_not_store");
        let updates = gi.drain_configstring_updates();
        assert!(updates.is_empty());
    }

    #[test]
    fn configstring_oob_index_ignored() {
        let gi = ServerGameImport::new();
        gi.configstring(MAX_CONFIGSTRINGS as i32 + 10, "oob");
        let updates = gi.drain_configstring_updates();
        assert!(updates.is_empty());
    }

    #[test]
    fn configstring_overwrite_returns_latest() {
        let gi = ServerGameImport::new();
        gi.configstring(3, "first");
        gi.configstring(3, "second");
        let updates = gi.drain_configstring_updates();
        let found = updates
            .iter()
            .find(|(i, _)| *i == 3)
            .map(|(_, v)| v.as_str());
        assert_eq!(found, Some("second"));
    }
}
