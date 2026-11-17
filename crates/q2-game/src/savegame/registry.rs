//! Callback names for save files (C `savegame/tables/gamefunc_list.h`).
//!
//! A save names each callback and monster move by its path in the crate,
//! since function addresses change between builds. `build.rs` generates the
//! tables from the sources.

use crate::entity::{
    BlockedFn, DieFn, MonsterCheckAttackFn, MonsterDodgeFn, MonsterMove, PainFn, ThinkFn, TouchFn,
    UseFn,
};

include!(concat!(env!("OUT_DIR"), "/save_tables.rs"));

/// A callback type a save can name. The aliases that share a signature
/// (`ThinkFn`, `PreThinkFn`, `EndMoveFn`, `MonsterStandFn`; `BlockedFn`,
/// `MonsterSightFn`) are one type and share a table.
pub trait Callback: Copy + 'static {
    /// Every registered function of this type, sorted by name.
    fn table() -> &'static [(&'static str, Self)];
    /// The function's address, for comparing pointers.
    fn addr(self) -> usize;
}

macro_rules! callback {
    ($($ty:ty => $table:ident),* $(,)?) => {$(
        impl Callback for $ty {
            fn table() -> &'static [(&'static str, Self)] {
                $table
            }
            fn addr(self) -> usize {
                self as usize
            }
        }
    )*};
}

callback! {
    ThinkFn => THINK_FN,
    MonsterCheckAttackFn => MONSTER_CHECK_ATTACK_FN,
    BlockedFn => BLOCKED_FN,
    UseFn => USE_FN,
    MonsterDodgeFn => MONSTER_DODGE_FN,
    PainFn => PAIN_FN,
    TouchFn => TOUCH_FN,
    DieFn => DIE_FN,
}

/// The registered name of `f`, or `None` for a function the tables lack.
pub fn name_of<F: Callback>(f: F) -> Option<&'static str> {
    let addr = f.addr();
    F::table()
        .iter()
        .find(|(_, g)| g.addr() == addr)
        .map(|&(name, _)| name)
}

/// The function registered as `name` with type `F`.
pub fn by_name<F: Callback>(name: &str) -> Option<F> {
    let table = F::table();
    table
        .binary_search_by(|(n, _)| (*n).cmp(name))
        .ok()
        .map(|i| table[i].1)
}

/// What identifies a monster move. `MonsterInfo::currentmove` holds a copy
/// of the static, so the static's own address is gone; its frames are
/// another static, whose address is unique, and two moves over the same
/// frames differ in their range or end function.
fn move_key(mv: &MonsterMove) -> (usize, usize, i32, i32, Option<usize>) {
    (
        mv.frames.as_ptr() as usize,
        mv.frames.len(),
        mv.firstframe,
        mv.lastframe,
        mv.endfunc.map(Callback::addr),
    )
}

/// The registered name of the static move `mv` copies.
pub fn move_name(mv: &MonsterMove) -> Option<&'static str> {
    let key = move_key(mv);
    MONSTER_MOVES
        .iter()
        .find(|(_, m)| move_key(m) == key)
        .map(|&(name, _)| name)
}

/// The static move registered as `name`.
pub fn move_by_name(name: &str) -> Option<&'static MonsterMove> {
    MONSTER_MOVES
        .binary_search_by(|(n, _)| (*n).cmp(name))
        .ok()
        .map(|i| MONSTER_MOVES[i].1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{DieFn, EntityKey, MonsterMove, ThinkFn, TouchFn};
    use crate::world::GameWorld;

    fn not_a_game_callback(_: &mut GameWorld, _: EntityKey) {}

    #[test]
    fn a_game_callback_is_found_by_name_and_by_pointer() {
        let done: ThinkFn = crate::func::move_done;
        assert_eq!(name_of(done), Some("func::move_done"));
        let found: ThinkFn = by_name("func::move_done").expect("registered");
        assert_eq!(found as usize, done as usize);
    }

    #[test]
    fn every_registered_callback_round_trips_by_name() {
        fn check<F: Callback>() {
            for &(name, f) in F::table() {
                assert_eq!(name_of(f), Some(name), "{name}");
                assert_eq!(by_name::<F>(name).map(F::addr), Some(f.addr()), "{name}");
            }
        }
        check::<ThinkFn>();
        check::<TouchFn>();
        check::<DieFn>();
    }

    #[test]
    fn an_unregistered_function_has_no_name() {
        let local: ThinkFn = not_a_game_callback;
        assert_eq!(name_of(local), None);
        assert!(by_name::<ThinkFn>("savegame::registry::tests::not_a_game_callback").is_none());
    }

    /// C's `insane_move_crawl` and `insane_move_runcrawl` are identical, so a
    /// copy of either names the first: both load back the same frames,
    /// range and end function.
    #[test]
    fn every_monster_move_loads_back_the_same_move() {
        for &(name, mv) in MONSTER_MOVES {
            let saved = move_name(&mv.clone()).expect("registered");
            let loaded = move_by_name(saved).expect("registered");
            assert_eq!(move_key(loaded), move_key(mv), "{name}");
        }
    }

    #[test]
    fn a_move_built_at_runtime_has_no_name() {
        let (_, soldier) = MONSTER_MOVES[0];
        let other_end = MonsterMove {
            endfunc: Some(not_a_game_callback),
            ..soldier.clone()
        };
        assert_eq!(move_name(&other_end), None);
        assert!(move_by_name("monster::nowhere::NO_MOVE").is_none());
    }

    #[test]
    fn a_name_of_another_signature_is_not_found() {
        assert!(by_name::<TouchFn>("func::move_done").is_none());
    }
}
