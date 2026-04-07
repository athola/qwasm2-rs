//! Console variable (CVar) system.
//!
//! Replaces the C linked-list `cvar_t` chain with a `HashMap`-based system.
//! CVars are key-value configuration variables used throughout the engine
//! (graphics settings, gameplay tweaks, network parameters, etc.).

use bitflags::bitflags;
use q2_shared::CVarHandle;
use std::collections::HashMap;

bitflags! {
    /// Flags controlling cvar behaviour.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct CVarFlags: u32 {
        /// Saved to config.cfg on shutdown.
        const ARCHIVE    = 1 << 0;
        /// Sent to server on connect/change.
        const USERINFO   = 1 << 1;
        /// Sent in server info responses.
        const SERVERINFO = 1 << 2;
        /// Cannot be changed by the user.
        const NOSET      = 1 << 3;
        /// Value change is deferred until `apply_latched()`.
        const LATCH      = 1 << 4;
    }
}

/// Internal storage for a single console variable.
#[derive(Debug, Clone)]
struct CVar {
    name: String,
    string: String,
    value: f32,
    default_value: String,
    flags: CVarFlags,
    latched_string: Option<String>,
    modified: bool,
}

/// Central registry for all console variables.
pub struct CVarSystem {
    vars: Vec<CVar>,
    name_to_index: HashMap<String, usize>,
    userinfo_modified: bool,
}

impl CVarSystem {
    /// Create an empty cvar system.
    pub fn new() -> Self {
        Self {
            vars: Vec::new(),
            name_to_index: HashMap::new(),
            userinfo_modified: false,
        }
    }

    /// Get or create a cvar. If it already exists, `flags` are OR-ed in but
    /// the value is left unchanged.
    pub fn get(&mut self, name: &str, default_value: &str, flags: CVarFlags) -> CVarHandle {
        if let Some(&index) = self.name_to_index.get(name) {
            self.vars[index].flags |= flags;
            return CVarHandle::from_raw(index);
        }

        let index = self.vars.len();
        let value = default_value.parse::<f32>().unwrap_or(0.0);
        self.vars.push(CVar {
            name: name.to_owned(),
            string: default_value.to_owned(),
            value,
            default_value: default_value.to_owned(),
            flags,
            latched_string: None,
            modified: false,
        });
        self.name_to_index.insert(name.to_owned(), index);
        CVarHandle::from_raw(index)
    }

    /// Set a cvar by name. Respects `NOSET` and `LATCH` flags.
    /// If the cvar does not exist it is created with empty flags.
    pub fn set(&mut self, name: &str, value: &str) {
        let index = match self.name_to_index.get(name) {
            Some(&i) => i,
            None => {
                // Create with empty flags, then set.
                let handle = self.get(name, value, CVarFlags::empty());
                // Already has the right value from `get`.
                self.vars[handle.raw()].modified = true;
                return;
            }
        };

        let cvar = &mut self.vars[index];

        if cvar.flags.contains(CVarFlags::NOSET) {
            return;
        }

        if cvar.flags.contains(CVarFlags::LATCH) {
            cvar.latched_string = Some(value.to_owned());
            return;
        }

        Self::apply_value(cvar, value);

        if cvar.flags.contains(CVarFlags::USERINFO) {
            self.userinfo_modified = true;
        }
    }

    /// Force-set a cvar, ignoring `NOSET` and `LATCH` flags.
    /// If the cvar does not exist it is created with empty flags.
    pub fn force_set(&mut self, name: &str, value: &str) {
        let index = match self.name_to_index.get(name) {
            Some(&i) => i,
            None => {
                let handle = self.get(name, value, CVarFlags::empty());
                self.vars[handle.raw()].modified = true;
                return;
            }
        };

        let cvar = &mut self.vars[index];
        cvar.latched_string = None;
        Self::apply_value(cvar, value);

        if cvar.flags.contains(CVarFlags::USERINFO) {
            self.userinfo_modified = true;
        }
    }

    /// Get the float value of a cvar.
    pub fn value(&self, handle: CVarHandle) -> f32 {
        self.vars.get(handle.raw()).map_or(0.0, |v| v.value)
    }

    /// Get the string value of a cvar.
    pub fn string(&self, handle: CVarHandle) -> &str {
        self.vars.get(handle.raw()).map_or("", |v| v.string.as_str())
    }

    /// Get the name of a cvar.
    pub fn name(&self, handle: CVarHandle) -> &str {
        self.vars.get(handle.raw()).map_or("", |v| v.name.as_str())
    }

    /// Get the flags of a cvar.
    pub fn flags(&self, handle: CVarHandle) -> CVarFlags {
        self.vars.get(handle.raw()).map_or(CVarFlags::empty(), |v| v.flags)
    }

    /// Get the default value string that was used when the cvar was first registered.
    pub fn default_value(&self, handle: CVarHandle) -> &str {
        self.vars.get(handle.raw()).map_or("", |v| v.default_value.as_str())
    }

    /// Apply all latched values (called at an appropriate point, e.g. map change).
    pub fn apply_latched(&mut self) {
        for cvar in &mut self.vars {
            if let Some(latched) = cvar.latched_string.take() {
                Self::apply_value(cvar, &latched);
            }
        }
    }

    /// Check whether any `USERINFO` cvar was modified since the last clear.
    pub fn userinfo_modified(&self) -> bool {
        self.userinfo_modified
    }

    /// Clear the userinfo-modified flag.
    pub fn clear_userinfo_modified(&mut self) {
        self.userinfo_modified = false;
    }

    /// Write all `ARCHIVE` cvars as `set name "value"` lines.
    pub fn write_archive(&self) -> String {
        let mut output = String::new();
        for cvar in &self.vars {
            if cvar.flags.contains(CVarFlags::ARCHIVE) {
                output.push_str(&format!("set {} \"{}\"\n", cvar.name, cvar.string));
            }
        }
        output
    }

    /// Look up a cvar by name, returning its handle if it exists.
    pub fn find(&self, name: &str) -> Option<CVarHandle> {
        self.name_to_index.get(name).map(|&i| CVarHandle::from_raw(i))
    }

    /// Tab-completion: return the first alphabetically sorted cvar name matching
    /// the given prefix.
    pub fn complete(&self, partial: &str) -> Option<String> {
        let mut matches: Vec<&str> = self
            .vars
            .iter()
            .filter(|c| c.name.starts_with(partial))
            .map(|c| c.name.as_str())
            .collect();
        matches.sort();
        matches.first().map(|s| (*s).to_owned())
    }

    /// Build a userinfo string from all `USERINFO` cvars.
    /// Uses the Quake backslash-delimited format: `\key\value\key\value`.
    pub fn userinfo_string(&self) -> String {
        Self::build_info_string(&self.vars, CVarFlags::USERINFO)
    }

    /// Build a serverinfo string from all `SERVERINFO` cvars.
    /// Uses the Quake backslash-delimited format: `\key\value\key\value`.
    pub fn serverinfo_string(&self) -> String {
        Self::build_info_string(&self.vars, CVarFlags::SERVERINFO)
    }

    // ---- private helpers ----

    /// Update the string and parsed float value on a cvar.
    fn apply_value(cvar: &mut CVar, value: &str) {
        cvar.string = value.to_owned();
        cvar.value = value.parse::<f32>().unwrap_or(0.0);
        cvar.modified = true;
    }

    /// Build a backslash-delimited info string from cvars that have the given flag.
    fn build_info_string(vars: &[CVar], flag: CVarFlags) -> String {
        let mut info = String::new();
        for cvar in vars {
            if cvar.flags.contains(flag) {
                info.push('\\');
                info.push_str(&cvar.name);
                info.push('\\');
                info.push_str(&cvar.string);
            }
        }
        info
    }
}

impl Default for CVarSystem {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_creates_cvar() {
        let mut cvars = CVarSystem::new();
        let handle = cvars.get("test_var", "42", CVarFlags::empty());
        assert_eq!(cvars.string(handle), "42");
        assert_eq!(cvars.value(handle), 42.0);
    }

    #[test]
    fn get_existing_returns_same_handle() {
        let mut cvars = CVarSystem::new();
        let h1 = cvars.get("test", "1", CVarFlags::empty());
        let h2 = cvars.get("test", "2", CVarFlags::empty());
        assert_eq!(h1, h2);
        assert_eq!(cvars.string(h1), "1"); // value not changed on re-get
    }

    #[test]
    fn get_existing_ors_flags() {
        let mut cvars = CVarSystem::new();
        cvars.get("test", "1", CVarFlags::ARCHIVE);
        cvars.get("test", "1", CVarFlags::USERINFO);
        let h = cvars.find("test").unwrap();
        let flags = cvars.flags(h);
        assert!(flags.contains(CVarFlags::ARCHIVE));
        assert!(flags.contains(CVarFlags::USERINFO));
    }

    #[test]
    fn set_changes_value() {
        let mut cvars = CVarSystem::new();
        let handle = cvars.get("test_var", "1", CVarFlags::empty());
        cvars.set("test_var", "2");
        assert_eq!(cvars.value(handle), 2.0);
        assert_eq!(cvars.string(handle), "2");
    }

    #[test]
    fn noset_prevents_change() {
        let mut cvars = CVarSystem::new();
        let handle = cvars.get("locked", "1", CVarFlags::NOSET);
        cvars.set("locked", "2");
        assert_eq!(cvars.value(handle), 1.0); // unchanged
    }

    #[test]
    fn force_set_overrides_noset() {
        let mut cvars = CVarSystem::new();
        let handle = cvars.get("locked", "1", CVarFlags::NOSET);
        cvars.force_set("locked", "2");
        assert_eq!(cvars.value(handle), 2.0);
    }

    #[test]
    fn latch_defers_change() {
        let mut cvars = CVarSystem::new();
        let handle = cvars.get("latched", "1", CVarFlags::LATCH);
        cvars.set("latched", "2");
        assert_eq!(cvars.value(handle), 1.0); // still old
        cvars.apply_latched();
        assert_eq!(cvars.value(handle), 2.0); // now updated
    }

    #[test]
    fn archive_flag_exports() {
        let mut cvars = CVarSystem::new();
        cvars.get("saved_var", "hello", CVarFlags::ARCHIVE);
        cvars.get("unsaved", "world", CVarFlags::empty());
        let output = cvars.write_archive();
        assert!(output.contains("set saved_var \"hello\""));
        assert!(!output.contains("unsaved"));
    }

    #[test]
    fn userinfo_modified_flag() {
        let mut cvars = CVarSystem::new();
        cvars.get("name", "player", CVarFlags::USERINFO);
        assert!(!cvars.userinfo_modified());
        cvars.set("name", "newname");
        assert!(cvars.userinfo_modified());
        cvars.clear_userinfo_modified();
        assert!(!cvars.userinfo_modified());
    }

    #[test]
    fn find_nonexistent_returns_none() {
        let cvars = CVarSystem::new();
        assert_eq!(cvars.find("nonexistent"), None);
    }

    #[test]
    fn complete_partial_name() {
        let mut cvars = CVarSystem::new();
        cvars.get("gl_brightness", "1.0", CVarFlags::empty());
        cvars.get("gl_contrast", "1.0", CVarFlags::empty());
        cvars.get("sv_maxclients", "8", CVarFlags::empty());
        // "gl_" should match one of the gl_ cvars
        let result = cvars.complete("gl_");
        assert!(result.is_some());
        let name = result.unwrap();
        assert!(name.starts_with("gl_"));
    }

    #[test]
    fn value_parses_non_numeric_as_zero() {
        let mut cvars = CVarSystem::new();
        let handle = cvars.get("text", "hello", CVarFlags::empty());
        assert_eq!(cvars.value(handle), 0.0);
    }

    #[test]
    fn userinfo_string_format() {
        let mut cvars = CVarSystem::new();
        cvars.get("name", "player1", CVarFlags::USERINFO);
        cvars.get("skin", "male/grunt", CVarFlags::USERINFO);
        cvars.get("secret", "hidden", CVarFlags::empty()); // not USERINFO
        let info = cvars.userinfo_string();
        assert!(info.contains("name"));
        assert!(info.contains("player1"));
        assert!(info.contains("skin"));
        assert!(!info.contains("secret"));
    }
}
