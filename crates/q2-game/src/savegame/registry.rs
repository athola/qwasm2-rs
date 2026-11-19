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
    /// The table's name in the saved-name list (`names_v2.txt`).
    const KIND: &'static str;
    /// Every registered function of this type, sorted by name.
    fn table() -> &'static [(&'static str, Self)];
    /// The function's address, for comparing pointers.
    fn addr(self) -> usize;
}

/// Code run once per callback type: a closure cannot be generic.
pub(crate) trait TableVisitor {
    fn visit<F: Callback>(&mut self);
}

macro_rules! callback {
    ($($ty:ty => $table:ident, $kind:literal),* $(,)?) => {
        /// Run `visitor` on every callback type.
        pub(crate) fn visit_tables(visitor: &mut impl TableVisitor) {
            $(visitor.visit::<$ty>();)*
        }
        $(impl Callback for $ty {
            const KIND: &'static str = $kind;
            fn table() -> &'static [(&'static str, Self)] {
                $table
            }
            fn addr(self) -> usize {
                self as usize
            }
        })*
    };
}

callback! {
    ThinkFn => THINK_FN, "think",
    MonsterCheckAttackFn => MONSTER_CHECK_ATTACK_FN, "checkattack",
    BlockedFn => BLOCKED_FN, "blocked",
    UseFn => USE_FN, "use",
    MonsterDodgeFn => MONSTER_DODGE_FN, "dodge",
    PainFn => PAIN_FN, "pain",
    TouchFn => TOUCH_FN, "touch",
    DieFn => DIE_FN, "die",
}

/// Makes a callback's body unlike every other, so no optimizer merges it
/// with one that does the same and its saved name stays its own. Each use
/// hands `black_box` a string naming the call site, which costs a register
/// move and changes nothing the game sees.
macro_rules! distinct_body {
    () => {
        ::std::hint::black_box(concat!(module_path!(), ":", line!()))
    };
}
pub(crate) use distinct_body;

/// Registered names a save cannot tell apart: callbacks at one address, or
/// monster moves with one `move_key`, each paired with the next such name.
/// Optimizers merge functions with identical bodies (LLVM in release
/// builds, then wasm-opt), and a save then records the first name for all
/// of them, which loads the wrong function once one body changes. Empty
/// when every name is exact; `distinct_body!` keeps a body apart.
pub fn aliased_names() -> Vec<(&'static str, &'static str)> {
    struct Aliases(Vec<(&'static str, &'static str)>);
    impl Aliases {
        fn collect<K: Ord>(&mut self, entries: impl Iterator<Item = (K, &'static str)>) {
            let mut sorted: Vec<_> = entries.collect();
            sorted.sort();
            for pair in sorted.windows(2) {
                if pair[0].0 == pair[1].0 {
                    self.0.push((pair[0].1, pair[1].1));
                }
            }
        }
    }
    impl TableVisitor for Aliases {
        fn visit<F: Callback>(&mut self) {
            self.collect(F::table().iter().map(|&(name, f)| (f.addr(), name)));
        }
    }
    let mut aliases = Aliases(Vec::new());
    visit_tables(&mut aliases);
    aliases.collect(MONSTER_MOVES.iter().map(|&(name, mv)| (move_key(mv), name)));
    aliases.0
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
    lookup(F::table(), name)
}

/// The entry named `name` in a table sorted by name.
fn lookup<T: Copy>(table: &[(&'static str, T)], name: &str) -> Option<T> {
    table
        .binary_search_by(|(n, _)| (*n).cmp(name))
        .ok()
        .map(|i| table[i].1)
}

/// What identifies a monster move. `MonsterInfo::currentmove` holds a copy
/// of the static, so the static's own address is gone; its frames are
/// another static, whose address is unique, and two moves over the same
/// frames must differ in their range or end function (`aliased_names`).
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
    lookup(MONSTER_MOVES, name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{EntityKey, MonsterMove, ThinkFn, TouchFn};
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
    fn a_callback_named_only_inside_a_macro_is_registered() {
        // `items::build_item_list` names it inside `vec![..]`.
        assert!(by_name::<ThinkFn>("player::weapon::weapon_blaster").is_some());
    }

    #[test]
    fn every_registered_callback_round_trips_by_name() {
        struct RoundTrip;
        impl TableVisitor for RoundTrip {
            fn visit<F: Callback>(&mut self) {
                for &(name, f) in F::table() {
                    assert_eq!(name_of(f), Some(name), "{name}");
                    assert_eq!(by_name::<F>(name).map(F::addr), Some(f.addr()), "{name}");
                }
            }
        }
        visit_tables(&mut RoundTrip);
    }

    /// Run by CI in release too, where LLVM merges identical bodies.
    #[test]
    fn no_two_registered_names_share_a_callback_or_move() {
        assert_eq!(aliased_names(), Vec::<(&str, &str)>::new());
    }

    #[test]
    fn an_unregistered_function_has_no_name() {
        let local: ThinkFn = not_a_game_callback;
        assert_eq!(name_of(local), None);
        assert!(by_name::<ThinkFn>("savegame::registry::tests::not_a_game_callback").is_none());
    }

    #[test]
    fn every_monster_move_loads_back_the_same_move() {
        for &(name, mv) in MONSTER_MOVES {
            let saved = move_name(&mv.clone()).expect("registered");
            assert_eq!(saved, name);
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

    /// Every name the tables held when save VERSION 2 was set, which a
    /// save written then may hold. Regenerate it with
    /// `Q2_WRITE_SAVE_NAMES=1 cargo test -p q2-game saved_names` when
    /// VERSION is bumped.
    const SAVED_NAMES: &str = include_str!("names_v2.txt");

    fn current_names() -> String {
        struct Names(String);
        impl TableVisitor for Names {
            fn visit<F: Callback>(&mut self) {
                for (name, _) in F::table() {
                    self.0.push_str(&format!("{} {name}\n", F::KIND));
                }
            }
        }
        let mut names = Names(String::new());
        visit_tables(&mut names);
        for (name, _) in MONSTER_MOVES {
            names.0.push_str(&format!("move {name}\n"));
        }
        names.0
    }

    #[test]
    fn every_saved_name_of_this_version_still_resolves() {
        // A rename or removal makes saves of this version fail to load:
        // bump VERSION and regenerate the list. Additions are harmless.
        if std::env::var_os("Q2_WRITE_SAVE_NAMES").is_some() {
            let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/savegame/names_v2.txt");
            std::fs::write(path, current_names()).unwrap();
            return;
        }
        let current = current_names();
        let current: std::collections::HashSet<&str> = current.lines().collect();
        let missing: Vec<&str> = SAVED_NAMES
            .lines()
            .filter(|line| !current.contains(line))
            .collect();
        assert!(missing.is_empty(), "bump VERSION: {missing:?}");
    }
}
