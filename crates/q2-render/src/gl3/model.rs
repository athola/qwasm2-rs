//! Model registry behind `Renderer::register_model` (C `Mod_ForName` /
//! `R_RegisterModel`). GL-free; `A` is the alias-model payload so the
//! registry logic can be tested without a GL context.
//!
//! Handles are `index + 1` so [`ModelHandle::NONE`] (0) is never a model.
//!
//! Each alias model remembers the registration sequence it was last
//! registered in (C `registration_sequence`), so `end_registration` can free
//! the ones the new map did not ask for.

use std::collections::{HashMap, HashSet};

use q2_render_api::ModelHandle;

pub enum Model<A> {
    /// Inline submodel `*n` of the loaded BSP (`n >= 1`).
    Brush {
        submodel: usize,
    },
    Alias(A),
    /// A `.sp2` sprite.
    Sprite(super::fx::SpriteModel),
}

struct Slot<A> {
    model: Model<A>,
    /// Registration sequence of the last `register` that returned it.
    sequence: u32,
}

pub struct ModelRegistry<A> {
    slots: Vec<Option<Slot<A>>>,
    by_name: HashMap<String, ModelHandle>,
    /// Names whose load failed this sequence; remembered so the failure is
    /// logged once per registration.
    failed: HashSet<String>,
    sequence: u32,
}

impl<A> Default for ModelRegistry<A> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            by_name: HashMap::new(),
            failed: HashSet::new(),
            sequence: 1,
        }
    }
}

/// Index `n` of an inline model name `*n`; world (`*0`) is not a valid entity model.
pub fn inline_index(name: &str) -> Option<usize> {
    name.strip_prefix('*')?
        .parse::<usize>()
        .ok()
        .filter(|&n| n >= 1)
}

impl<A> ModelRegistry<A> {
    /// Forget every model (renderer init and shutdown, C `Mod_FreeAll`).
    pub fn clear(&mut self) {
        let sequence = self.sequence;
        *self = Self::default();
        self.sequence = sequence;
    }

    /// Start a registration sequence (C `registration_sequence++` in
    /// `R_BeginRegistration`). Failed names are tried again, as C caches no
    /// failures.
    pub fn begin_sequence(&mut self) {
        self.sequence = self.sequence.wrapping_add(1);
        self.failed.clear();
    }

    #[cfg(test)]
    pub fn sequence(&self) -> u32 {
        self.sequence
    }

    /// Free alias models not registered in the current sequence (C
    /// `GL3_EndRegistration`). Inline models belong to the world and go with
    /// it in [`ModelRegistry::clear_brush`].
    pub fn free_unused(&mut self) {
        let sequence = self.sequence;
        self.retain(|slot| matches!(slot.model, Model::Brush { .. }) || slot.sequence == sequence);
    }

    /// Forget brush models only: a new BSP changes what `*n` means, while
    /// alias models are independent of the map.
    pub fn clear_brush(&mut self) {
        self.retain(|slot| !matches!(slot.model, Model::Brush { .. }));
        self.failed.clear();
    }

    /// Free the slots `keep` rejects and forget their names.
    fn retain(&mut self, keep: impl Fn(&Slot<A>) -> bool) {
        for slot in &mut self.slots {
            if slot.as_ref().is_some_and(|slot| !keep(slot)) {
                *slot = None;
            }
        }
        // Freed slots stay so a stale handle never names a later model; that
        // costs one slot per freed model.
        let slots = &self.slots;
        self.by_name.retain(|_, handle| {
            usize::try_from(handle.raw())
                .ok()
                .and_then(|raw| raw.checked_sub(1))
                .and_then(|index| slots.get(index))
                .is_some_and(Option::is_some)
        });
    }

    fn handle_for(index: usize) -> ModelHandle {
        ModelHandle::new(i32::try_from(index + 1).unwrap_or(i32::MAX))
    }

    fn index_of(handle: ModelHandle) -> Option<usize> {
        usize::try_from(handle.raw()).ok()?.checked_sub(1)
    }

    pub fn get(&self, handle: ModelHandle) -> Option<&Model<A>> {
        let index = Self::index_of(handle)?;
        self.slots.get(index)?.as_ref().map(|slot| &slot.model)
    }

    /// Handle for `name`, running `load` only the first time the name is seen.
    /// A failed load returns `NONE` and is logged once. Either way the model
    /// is marked as used by the current sequence.
    pub fn register(&mut self, name: &str, load: impl FnOnce() -> Option<Model<A>>) -> ModelHandle {
        let sequence = self.sequence;
        if let Some(&handle) = self.by_name.get(name) {
            if let Some(Some(slot)) = Self::index_of(handle).and_then(|i| self.slots.get_mut(i)) {
                slot.sequence = sequence;
            }
            return handle;
        }
        if self.failed.contains(name) {
            return ModelHandle::NONE;
        }
        match load() {
            Some(model) => {
                self.slots.push(Some(Slot { model, sequence }));
                let handle = Self::handle_for(self.slots.len() - 1);
                self.by_name.insert(name.to_string(), handle);
                handle
            }
            None => {
                tracing::warn!("GL3: can't register model {name}");
                self.failed.insert(name.to_string());
                ModelHandle::NONE
            }
        }
    }

    /// Register inline model `*n` of a BSP with `model_count` models
    /// (`None`: no BSP loaded yet, so nothing can be validated).
    pub fn register_inline(&mut self, name: &str, model_count: Option<usize>) -> ModelHandle {
        let Some(count) = model_count else {
            tracing::warn!("GL3: inline model {name} requested before a map is loaded");
            return ModelHandle::NONE;
        };
        self.register(name, || {
            let submodel = inline_index(name).filter(|&n| n < count)?;
            Some(Model::Brush { submodel })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    type Registry = ModelRegistry<&'static str>;

    #[test]
    fn same_name_returns_the_same_handle_without_reloading() {
        let mut registry = Registry::default();
        let loads = Cell::new(0);
        let load = || {
            loads.set(loads.get() + 1);
            Some(Model::Alias("soldier"))
        };
        let first = registry.register("models/soldier/tris.md2", load);
        let second = registry.register("models/soldier/tris.md2", load);
        assert_eq!(first, second);
        assert_ne!(first, ModelHandle::NONE);
        assert_eq!(loads.get(), 1);
    }

    #[test]
    fn distinct_names_get_distinct_nonzero_handles() {
        let mut registry = Registry::default();
        let a = registry.register("a.md2", || Some(Model::Alias("a")));
        let b = registry.register("b.md2", || Some(Model::Alias("b")));
        assert_ne!(a, b);
        assert!(a.raw() > 0 && b.raw() > 0);
        assert!(matches!(registry.get(b), Some(Model::Alias("b"))));
    }

    #[test]
    fn failed_load_is_none_and_not_retried() {
        let mut registry = Registry::default();
        let attempts = Cell::new(0);
        let load = || {
            attempts.set(attempts.get() + 1);
            None
        };
        assert_eq!(registry.register("missing.md2", load), ModelHandle::NONE);
        assert_eq!(registry.register("missing.md2", load), ModelHandle::NONE);
        assert_eq!(attempts.get(), 1);
    }

    #[test]
    fn get_rejects_none_and_unknown_handles() {
        let mut registry = Registry::default();
        registry.register("a.md2", || Some(Model::Alias("a")));
        assert!(registry.get(ModelHandle::NONE).is_none());
        assert!(registry.get(ModelHandle::new(2)).is_none());
        assert!(registry.get(ModelHandle::new(-3)).is_none());
    }

    #[test]
    fn inline_names_parse_only_positive_indices() {
        assert_eq!(inline_index("*1"), Some(1));
        assert_eq!(inline_index("*17"), Some(17));
        assert_eq!(inline_index("*0"), None);
        assert_eq!(inline_index("*"), None);
        assert_eq!(inline_index("*x"), None);
        assert_eq!(inline_index("*-1"), None);
        assert_eq!(inline_index("1"), None);
    }

    #[test]
    fn inline_models_are_bounds_checked_against_the_bsp() {
        let mut registry = Registry::default();
        // BSP with models 0..3: *1 and *2 exist, *3 and *0 do not.
        let valid = registry.register_inline("*2", Some(3));
        assert!(matches!(
            registry.get(valid),
            Some(Model::Brush { submodel: 2 })
        ));
        assert_eq!(registry.register_inline("*3", Some(3)), ModelHandle::NONE);
        assert_eq!(registry.register_inline("*0", Some(3)), ModelHandle::NONE);
        assert_eq!(registry.register_inline("*1", None), ModelHandle::NONE);
        // The same inline name keeps its handle.
        assert_eq!(registry.register_inline("*2", Some(3)), valid);
    }

    #[test]
    fn clear_brush_keeps_alias_models_and_frees_inline_names() {
        let mut registry = Registry::default();
        let alias = registry.register("a.md2", || Some(Model::Alias("a")));
        let brush = registry.register_inline("*1", Some(2));
        registry.clear_brush();
        assert!(registry.get(brush).is_none());
        assert!(matches!(registry.get(alias), Some(Model::Alias("a"))));
        assert_eq!(registry.register("a.md2", || None), alias);
        // After a new map the same name registers afresh.
        let again = registry.register_inline("*1", Some(2));
        assert_ne!(again, brush);
        assert!(matches!(
            registry.get(again),
            Some(Model::Brush { submodel: 1 })
        ));
    }

    #[test]
    fn free_unused_drops_alias_models_not_registered_since_begin_sequence() {
        let mut registry = Registry::default();
        let old = registry.register("old.md2", || Some(Model::Alias("old")));
        let kept = registry.register("kept.md2", || Some(Model::Alias("kept")));
        let brush = registry.register_inline("*1", Some(2));
        registry.begin_sequence();
        assert_eq!(registry.register("kept.md2", || None), kept);
        registry.free_unused();
        assert!(registry.get(old).is_none());
        assert!(matches!(registry.get(kept), Some(Model::Alias("kept"))));
        // Inline models belong to the world and go with it, not the sweep.
        assert!(registry.get(brush).is_some());
        // A freed name loads afresh under a new handle.
        let again = registry.register("old.md2", || Some(Model::Alias("old")));
        assert_ne!(again, old);
        assert!(matches!(registry.get(again), Some(Model::Alias("old"))));
    }

    #[test]
    fn failed_load_is_retried_in_a_new_sequence() {
        // C Mod_ForName caches no failures; this registry remembers one only
        // for the current registration so it is logged once per map load.
        let mut registry = Registry::default();
        assert_eq!(registry.register("late.md2", || None), ModelHandle::NONE);
        registry.begin_sequence();
        let late = registry.register("late.md2", || Some(Model::Alias("late")));
        assert!(matches!(registry.get(late), Some(Model::Alias("late"))));
    }

    #[test]
    fn free_unused_keeps_models_registered_in_the_current_sequence() {
        let mut registry = Registry::default();
        registry.begin_sequence();
        let model = registry.register("a.md2", || Some(Model::Alias("a")));
        registry.free_unused();
        assert!(registry.get(model).is_some());
    }

    #[test]
    fn clear_forgets_everything() {
        let mut registry = Registry::default();
        let alias = registry.register("a.md2", || Some(Model::Alias("a")));
        registry.clear();
        assert!(registry.get(alias).is_none());
    }
}
