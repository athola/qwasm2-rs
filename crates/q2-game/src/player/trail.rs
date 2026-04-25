//! Player trail — breadcrumb path for monster AI pathfinding.
//! C ref: `player/trail.c` (175 lines)

use q2_shared::types::Vec3f;

/// Maximum trail nodes stored.
const TRAIL_LENGTH: usize = 8;

/// Ring buffer of player positions for monster AI to follow.
#[derive(Debug, Clone)]
pub struct PlayerTrail {
    nodes: [Vec3f; TRAIL_LENGTH],
    head: usize,
    count: usize,
}

impl Default for PlayerTrail {
    fn default() -> Self {
        Self {
            nodes: [Vec3f::ZERO; TRAIL_LENGTH],
            head: 0,
            count: 0,
        }
    }
}

impl PlayerTrail {
    /// Add a new trail node at the player's current position.
    pub fn add(&mut self, origin: Vec3f) {
        self.nodes[self.head] = origin;
        self.head = (self.head + 1) % TRAIL_LENGTH;
        if self.count < TRAIL_LENGTH {
            self.count += 1;
        }
    }

    /// Get the most recent trail node.
    pub fn last_spot(&self) -> Option<Vec3f> {
        if self.count == 0 {
            return None;
        }
        let idx = if self.head == 0 {
            TRAIL_LENGTH - 1
        } else {
            self.head - 1
        };
        Some(self.nodes[idx])
    }

    /// Pick a trail node near the given position (for monster pathfinding).
    pub fn pick_near(&self, origin: Vec3f) -> Option<Vec3f> {
        if self.count == 0 {
            return None;
        }

        let mut best = None;
        let mut best_dist = f32::MAX;

        for i in 0..self.count {
            let idx = (self.head + TRAIL_LENGTH - 1 - i) % TRAIL_LENGTH;
            let dist = (self.nodes[idx] - origin).length();
            if dist < best_dist {
                best_dist = dist;
                best = Some(self.nodes[idx]);
            }
        }

        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trail_add_and_last() {
        let mut trail = PlayerTrail::default();
        trail.add(Vec3f::new(10.0, 0.0, 0.0));
        trail.add(Vec3f::new(20.0, 0.0, 0.0));

        assert_eq!(trail.last_spot(), Some(Vec3f::new(20.0, 0.0, 0.0)));
    }

    #[test]
    fn trail_wraps_around() {
        let mut trail = PlayerTrail::default();
        for i in 0..20 {
            trail.add(Vec3f::new(i as f32, 0.0, 0.0));
        }

        // Should still work after wrap.
        assert!(trail.last_spot().is_some());
        assert_eq!(trail.count, TRAIL_LENGTH);
    }

    #[test]
    fn trail_pick_near() {
        let mut trail = PlayerTrail::default();
        trail.add(Vec3f::new(100.0, 0.0, 0.0));
        trail.add(Vec3f::new(200.0, 0.0, 0.0));
        trail.add(Vec3f::new(300.0, 0.0, 0.0));

        let near = trail.pick_near(Vec3f::new(190.0, 0.0, 0.0));
        assert_eq!(near, Some(Vec3f::new(200.0, 0.0, 0.0)));
    }

    #[test]
    fn trail_empty() {
        let trail = PlayerTrail::default();
        assert!(trail.last_spot().is_none());
        assert!(trail.pick_near(Vec3f::ZERO).is_none());
    }
}
