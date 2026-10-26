//! Console variable (CVar) system.
//!
//! Replaces the C linked-list `cvar_t` chain with a `HashMap`-based system.
//! CVars are key-value configuration variables used throughout the engine
//! (graphics settings, gameplay tweaks, network parameters, etc.).

use bitflags::bitflags;
use q2_shared::CVarHandle;
use std::collections::HashMap;

/// C `MAX_INFO_KEY` / `MAX_INFO_VALUE`: keys and values must be shorter.
const MAX_INFO_KEY: usize = 64;
/// C `MAX_INFO_STRING`: the whole info string must stay below this length.
const MAX_INFO_STRING: usize = 512;

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
        let value = parse_value(default_value);
        self.vars.push(CVar {
            name: name.to_owned(),
            string: default_value.to_owned(),
            value,
            default_value: default_value.to_owned(),
            flags,
            latched_string: None,
            // C Cvar_Get marks a freshly created cvar modified.
            modified: true,
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

        if !Self::info_value_allowed(cvar, value) {
            tracing::warn!("invalid info cvar value for {}", cvar.name);
            return;
        }

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

        if !Self::info_value_allowed(cvar, value) {
            tracing::warn!("invalid info cvar value for {}", cvar.name);
            return;
        }

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
        self.vars
            .get(handle.raw())
            .map_or("", |v| v.string.as_str())
    }

    /// Get the name of a cvar.
    pub fn name(&self, handle: CVarHandle) -> &str {
        self.vars.get(handle.raw()).map_or("", |v| v.name.as_str())
    }

    /// Get the flags of a cvar.
    pub fn flags(&self, handle: CVarHandle) -> CVarFlags {
        self.vars
            .get(handle.raw())
            .map_or(CVarFlags::empty(), |v| v.flags)
    }

    /// Get the default value string that was used when the cvar was first registered.
    pub fn default_value(&self, handle: CVarHandle) -> &str {
        self.vars
            .get(handle.raw())
            .map_or("", |v| v.default_value.as_str())
    }

    /// Whether the cvar changed since the last `clear_modified` (C
    /// `cvar->modified`), for change detection in renderer/network code.
    pub fn modified(&self, handle: CVarHandle) -> bool {
        self.vars.get(handle.raw()).is_some_and(|v| v.modified)
    }

    /// Acknowledge a change reported by `modified`.
    pub fn clear_modified(&mut self, handle: CVarHandle) {
        if let Some(v) = self.vars.get_mut(handle.raw()) {
            v.modified = false;
        }
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
                // The command tokenizer has no escape syntax (like C
                // COM_Parse), so a quote or newline in the value would end
                // the quoted token and let the rest run as commands when the
                // config is exec'd. C writes the value raw; we drop them.
                let value: String = cvar
                    .string
                    .chars()
                    .filter(|c| !matches!(c, '"' | '\n' | '\r'))
                    .collect();
                output.push_str(&format!("set {} \"{}\"\n", cvar.name, value));
            }
        }
        output
    }

    /// Look up a cvar by name, returning its handle if it exists.
    pub fn find(&self, name: &str) -> Option<CVarHandle> {
        self.name_to_index
            .get(name)
            .map(|&i| CVarHandle::from_raw(i))
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
        cvar.value = parse_value(value);
        cvar.modified = true;
    }

    /// C `Cvar_InfoValidate`, applied only to USERINFO/SERVERINFO cvars:
    /// these characters would forge keys or break the info string.
    fn info_value_allowed(cvar: &CVar, value: &str) -> bool {
        !cvar
            .flags
            .intersects(CVarFlags::USERINFO | CVarFlags::SERVERINFO)
            || !value.contains(['\\', '"', ';'])
    }

    /// Build a backslash-delimited info string from cvars that have the given
    /// flag, applying each pair the way C `Info_SetValueForKey` does.
    fn build_info_string(vars: &[CVar], flag: CVarFlags) -> String {
        let mut info = String::new();
        for cvar in vars.iter().filter(|c| c.flags.contains(flag)) {
            let (key, value) = (cvar.name.as_str(), cvar.string.as_str());
            if key.contains(['\\', '"', ';']) || value.contains(['\\', '"']) {
                tracing::warn!("Can't use keys or values with a \\ or \" ({key})");
                continue;
            }
            if key.len() >= MAX_INFO_KEY || value.len() >= MAX_INFO_KEY {
                tracing::warn!("Keys and values must be < 64 characters ({key})");
                continue;
            }
            if value.is_empty() {
                continue;
            }
            let pair = format!("\\{key}\\{value}");
            if pair.len() + info.len() >= MAX_INFO_STRING {
                tracing::warn!("Info string length exceeded ({key})");
                continue;
            }
            // Only printable ASCII survives, high bit stripped first.
            info.extend(
                pair.bytes()
                    .map(|b| b & 0x7f)
                    .filter(|b| (32..127).contains(b))
                    .map(char::from),
            );
        }
        info
    }
}

/// Parse a cvar's float value like C `(float)strtod(s, NULL)`: leading
/// whitespace is skipped and the longest decimal prefix is used, so `" 3"`
/// is 3 and `"1.5abc"` is 1.5. Non-finite results (`nan`, `inf`, overflow)
/// become 0 so they cannot poison physics cvars. Hex floats are not parsed.
fn parse_value(text: &str) -> f32 {
    let text = text.trim_start_matches(|c: char| c.is_ascii_whitespace());
    let bytes = text.as_bytes();
    let digits_from = |mut i: usize| {
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        i
    };

    let mut end = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let int_end = digits_from(end);
    let mut mantissa_digits = int_end - end;
    end = int_end;
    if bytes.get(end) == Some(&b'.') {
        let frac_end = digits_from(end + 1);
        mantissa_digits += frac_end - (end + 1);
        end = frac_end;
    }
    if mantissa_digits == 0 {
        return 0.0;
    }
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let mut exp = end + 1;
        if matches!(bytes.get(exp), Some(b'+' | b'-')) {
            exp += 1;
        }
        let exp_end = digits_from(exp);
        if exp_end > exp {
            end = exp_end;
        }
    }

    text[..end]
        .parse::<f64>()
        .map(|v| v as f32)
        .ok()
        .filter(|v| v.is_finite())
        .unwrap_or(0.0)
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
    fn set_rejects_info_separators_in_userinfo_value() {
        // C Cvar_InfoValidate: `\`, `"` and `;` would forge extra keys or
        // break the info string, so the old value is kept.
        let mut cvars = CVarSystem::new();
        let h = cvars.get("name", "player", CVarFlags::USERINFO);
        for bad in ["x\\ip\\1.2.3.4", "a\"b", "a;b"] {
            cvars.set("name", bad);
            assert_eq!(cvars.string(h), "player", "accepted {bad:?}");
            cvars.force_set("name", bad);
            assert_eq!(cvars.string(h), "player", "force accepted {bad:?}");
        }
        let s = cvars.get("hostname", "srv", CVarFlags::SERVERINFO);
        cvars.set("hostname", "a\\b");
        assert_eq!(cvars.string(s), "srv");
        // Non-info cvars may hold any text.
        let plain = cvars.get("plain", "", CVarFlags::empty());
        cvars.set("plain", "a;b");
        assert_eq!(cvars.string(plain), "a;b");
    }

    #[test]
    fn userinfo_string_drops_values_info_set_value_for_key_rejects() {
        let mut cvars = CVarSystem::new();
        cvars.get("name", "player1", CVarFlags::USERINFO);
        cvars.get("evil", "a\\ip\\1.2.3.4", CVarFlags::USERINFO);
        cvars.get("quote", "a\"b", CVarFlags::USERINFO);
        cvars.get("empty", "", CVarFlags::USERINFO);
        cvars.get("long", &"v".repeat(64), CVarFlags::USERINFO);
        assert_eq!(cvars.userinfo_string(), "\\name\\player1");
    }

    #[test]
    fn userinfo_string_strips_non_printable_bytes() {
        let mut cvars = CVarSystem::new();
        cvars.get("name", "a\tb\u{7f}c", CVarFlags::USERINFO);
        assert_eq!(cvars.userinfo_string(), "\\name\\abc");
    }

    #[test]
    fn userinfo_string_stops_below_max_info_string() {
        let mut cvars = CVarSystem::new();
        for i in 0..20 {
            cvars.get(&format!("key{i:02}"), &"v".repeat(40), CVarFlags::USERINFO);
        }
        let info = cvars.userinfo_string();
        assert!(info.len() < 512, "len {}", info.len());
        // Each pair is `\keyNN\` + 40 bytes = 47 bytes; 10 fit below 512.
        assert_eq!(info.matches('\\').count(), 2 * 10);
    }

    #[test]
    fn write_archive_cannot_inject_commands_through_quotes_or_newlines() {
        let mut cvars = CVarSystem::new();
        cvars.get("evil", "x\" ; quit \"", CVarFlags::ARCHIVE);
        cvars.get("multi", "a\nquit", CVarFlags::ARCHIVE);
        let output = cvars.write_archive();
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 2, "{output:?}");
        for line in lines {
            assert_eq!(crate::cmd::tokenize(line).len(), 3, "{line:?}");
        }
    }

    #[test]
    fn value_parses_numeric_prefix_like_strtod() {
        let mut cvars = CVarSystem::new();
        let cases = [
            (" 3", 3.0),
            ("1.5abc", 1.5),
            ("-2", -2.0),
            (".5", 0.5),
            ("1e3x", 1000.0),
            ("1e", 1.0),
            ("-", 0.0),
            ("nan", 0.0),
            ("inf", 0.0),
            ("1e40", 0.0),
        ];
        for (i, (text, expected)) in cases.iter().enumerate() {
            let h = cvars.get(&format!("v{i}"), text, CVarFlags::empty());
            assert_eq!(cvars.value(h), *expected, "default {text:?}");
            cvars.set(&format!("v{i}"), text);
            assert_eq!(cvars.value(h), *expected, "set {text:?}");
        }
    }

    #[test]
    fn modified_flag_tracks_changes_until_cleared() {
        let mut cvars = CVarSystem::new();
        let h = cvars.get("gl_mode", "3", CVarFlags::empty());
        // C Cvar_Get marks a new cvar modified.
        assert!(cvars.modified(h));
        cvars.clear_modified(h);
        assert!(!cvars.modified(h));
        cvars.set("gl_mode", "4");
        assert!(cvars.modified(h));
    }

    #[test]
    fn set_on_unknown_creates_cvar_with_value() {
        let mut cvars = CVarSystem::new();
        cvars.set("fresh", "7");
        let h = cvars.find("fresh").unwrap();
        assert_eq!(cvars.value(h), 7.0);
        assert_eq!(cvars.name(h), "fresh");
        assert_eq!(cvars.default_value(h), "7");
    }

    #[test]
    fn force_set_discards_pending_latch() {
        let mut cvars = CVarSystem::new();
        let h = cvars.get("maxclients", "1", CVarFlags::LATCH);
        cvars.set("maxclients", "8");
        cvars.force_set("maxclients", "4");
        cvars.apply_latched();
        assert_eq!(cvars.value(h), 4.0);
    }

    #[test]
    fn serverinfo_string_lists_only_serverinfo_cvars() {
        let mut cvars = CVarSystem::new();
        cvars.get("hostname", "q2", CVarFlags::SERVERINFO);
        cvars.get("name", "p", CVarFlags::USERINFO);
        assert_eq!(cvars.serverinfo_string(), "\\hostname\\q2");
    }

    #[test]
    fn complete_returns_alphabetically_first_match() {
        let mut cvars = CVarSystem::new();
        cvars.get("gl_z", "1", CVarFlags::empty());
        cvars.get("gl_a", "1", CVarFlags::empty());
        assert_eq!(cvars.complete("gl_").as_deref(), Some("gl_a"));
        assert_eq!(cvars.complete("zz"), None);
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
