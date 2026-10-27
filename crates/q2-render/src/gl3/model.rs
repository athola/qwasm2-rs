//! Model registry behind `Renderer::register_model` (C `Mod_ForName` /
//! `R_RegisterModel`). GL-free; `A` is the alias-model payload so the
//! registry logic can be tested without a GL context.
//!
//! Handles are `index + 1` so [`ModelHandle::NONE`] (0) is never a model.

use std::collections::{HashMap, HashSet};

use q2_render_api::ModelHandle;

pub enum Model<A> {
    /// Inline submodel `*n` of the loaded BSP (`n >= 1`).
    Brush {
        submodel: usize,
    },
    Alias(A),
}

pub struct ModelRegistry<A> {
    slots: Vec<Option<Model<A>>>,
    by_name: HashMap<String, ModelHandle>,
    /// Names whose load failed; remembered so the failure is logged once.
    failed: HashSet<String>,
}

impl<A> Default for ModelRegistry<A> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            by_name: HashMap::new(),
            failed: HashSet::new(),
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
    /// Forget every model (C `R_BeginRegistration` frees the old map's models).
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Forget brush models only: a new BSP changes what `*n` means, while
    /// alias models are independent of the map.
    pub fn clear_brush(&mut self) {
        for slot in &mut self.slots {
            if matches!(slot, Some(Model::Brush { .. })) {
                *slot = None;
            }
        }
        let live: Vec<ModelHandle> = self
            .slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.is_some())
            .map(|(index, _)| Self::handle_for(index))
            .collect();
        self.by_name.retain(|_, handle| live.contains(handle));
        self.failed.clear();
    }

    fn handle_for(index: usize) -> ModelHandle {
        ModelHandle::new(i32::try_from(index + 1).unwrap_or(i32::MAX))
    }

    pub fn get(&self, handle: ModelHandle) -> Option<&Model<A>> {
        let index = usize::try_from(handle.raw()).ok()?.checked_sub(1)?;
        self.slots.get(index)?.as_ref()
    }

    /// Handle for `name`, running `load` only the first time the name is seen.
    /// A failed load returns `NONE` and is logged once.
    pub fn register(&mut self, name: &str, load: impl FnOnce() -> Option<Model<A>>) -> ModelHandle {
        if let Some(&handle) = self.by_name.get(name) {
            return handle;
        }
        if self.failed.contains(name) {
            return ModelHandle::NONE;
        }
        match load() {
            Some(model) => {
                self.slots.push(Some(model));
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
    fn clear_forgets_everything() {
        let mut registry = Registry::default();
        let alias = registry.register("a.md2", || Some(Model::Alias("a")));
        registry.clear();
        assert!(registry.get(alias).is_none());
    }
}
