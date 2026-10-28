//! Which entities a client can see: the cluster list `SV_LinkEdict` keeps
//! per entity, `SV_FatPVS`, and the PVS test of `SV_BuildClientFrame`.

/// C `MAX_ENT_CLUSTERS`.
pub const MAX_ENT_CLUSTERS: usize = 16;
/// C `MAX_TOTAL_ENT_LEAFS`: leafs `SV_LinkEdict` asks `CM_BoxLeafnums` for.
pub const MAX_TOTAL_ENT_LEAFS: usize = 128;

/// Where a linked entity sits for visibility (C `num_clusters`,
/// `clusternums` and `headnode`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntityClusters {
    /// The distinct clusters its box touches; empty is never visible.
    List(Vec<i32>),
    /// Too many leafs or clusters to list: the node whose subtree holds them.
    Headnode(i32),
}

/// C `SV_LinkEdict`'s cluster list from the clusters of the leafs the
/// entity's box touches (`CM_BoxLeafnums` capped at `MAX_TOTAL_ENT_LEAFS`)
/// and the first node that split the box.
pub fn entity_clusters(leaf_clusters: &[i32], topnode: i32) -> EntityClusters {
    if leaf_clusters.len() >= MAX_TOTAL_ENT_LEAFS {
        return EntityClusters::Headnode(topnode);
    }
    let mut list = Vec::new();
    for &cluster in leaf_clusters {
        if cluster == -1 || list.contains(&cluster) {
            continue;
        }
        if list.len() == MAX_ENT_CLUSTERS {
            return EntityClusters::Headnode(topnode);
        }
        list.push(cluster);
    }
    EntityClusters::List(list)
}

/// C `SV_FatPVS`: the union of the PVS rows of the clusters around the eye,
/// each distinct cluster fetched once. No clusters (C's fatal error) is an
/// empty row, in which nothing is visible.
pub fn fat_pvs(leaf_clusters: &[i32], cluster_pvs: impl Fn(i32) -> Vec<u8>) -> Vec<u8> {
    let mut fat: Vec<u8> = Vec::new();
    for (index, &cluster) in leaf_clusters.iter().enumerate() {
        if leaf_clusters[..index].contains(&cluster) {
            continue;
        }
        let row = cluster_pvs(cluster);
        if fat.is_empty() {
            fat = row;
        } else {
            for (bits, more) in fat.iter_mut().zip(row) {
                *bits |= more;
            }
        }
    }
    fat
}

/// The PVS test of C `SV_BuildClientFrame` for a non-beam entity.
pub fn in_pvs(
    clusters: &EntityClusters,
    pvs: &[u8],
    headnode_visible: impl Fn(i32, &[u8]) -> bool,
) -> bool {
    match clusters {
        EntityClusters::Headnode(node) => headnode_visible(*node, pvs),
        EntityClusters::List(list) => list.iter().any(|&cluster| {
            usize::try_from(cluster).is_ok_and(|cluster| {
                pvs.get(cluster >> 3)
                    .is_some_and(|bits| bits & (1 << (cluster & 7)) != 0)
            })
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clusters_skip_outside_leafs_and_duplicates() {
        assert_eq!(
            entity_clusters(&[3, -1, 3, 5], 7),
            EntityClusters::List(vec![3, 5])
        );
    }

    #[test]
    fn too_many_clusters_or_leafs_fall_back_to_the_headnode() {
        let many: Vec<i32> = (0..=MAX_ENT_CLUSTERS as i32).collect();
        assert_eq!(entity_clusters(&many, 7), EntityClusters::Headnode(7));
        let exactly: Vec<i32> = (0..MAX_ENT_CLUSTERS as i32).collect();
        assert!(
            matches!(entity_clusters(&exactly, 7), EntityClusters::List(l) if l.len() == MAX_ENT_CLUSTERS)
        );
        let leafs = vec![1; MAX_TOTAL_ENT_LEAFS];
        assert_eq!(entity_clusters(&leafs, 9), EntityClusters::Headnode(9));
    }

    #[test]
    fn fat_pvs_ors_each_distinct_cluster_once() {
        let pvs = |cluster: i32| match cluster {
            0 => vec![0b0001, 0],
            2 => vec![0b0100, 0x80],
            _ => panic!("cluster {cluster} asked for twice or unexpectedly"),
        };
        assert_eq!(fat_pvs(&[0, 2], pvs), vec![0b0101, 0x80]);
        let once = std::cell::Cell::new(0);
        let counted = |_: i32| {
            once.set(once.get() + 1);
            vec![1]
        };
        fat_pvs(&[4, 4, 4], counted);
        assert_eq!(once.get(), 1);
        assert!(fat_pvs(&[], pvs).is_empty());
    }

    #[test]
    fn listed_entity_is_visible_when_any_cluster_bit_is_set() {
        let pvs = [0b0000_0100, 0b0000_0010]; // clusters 2 and 9
        let no_headnode = |_: i32, _: &[u8]| panic!("list entities never walk the tree");
        assert!(in_pvs(&EntityClusters::List(vec![1, 9]), &pvs, no_headnode));
        assert!(!in_pvs(
            &EntityClusters::List(vec![1, 3]),
            &pvs,
            no_headnode
        ));
        assert!(!in_pvs(&EntityClusters::List(vec![]), &pvs, no_headnode));
        assert!(
            !in_pvs(&EntityClusters::List(vec![40]), &pvs, no_headnode),
            "past the row"
        );
    }

    #[test]
    fn headnode_entity_asks_the_tree() {
        let pvs = [0u8];
        assert!(in_pvs(&EntityClusters::Headnode(5), &pvs, |node, _| node == 5));
        assert!(!in_pvs(&EntityClusters::Headnode(6), &pvs, |node, _| node == 5));
    }
}
