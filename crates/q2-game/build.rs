//! Generates the save-file name tables (C `savegame/tables/gamefunc_list.h`
//! and `gamemmove_list.h`, which C also generates from its sources).
//!
//! A function is registered when its signature is one of the entity
//! callback types and the crate uses it as a value somewhere (assigns or
//! passes it rather than calls it). Every `MonsterMove` static is
//! registered. The generated paths must be visible from the crate root, so
//! a private callback fails to compile until it is made `pub(crate)`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use quote::ToTokens;
use syn::visit::Visit;

/// Normalized signature -> callback type alias in `crate::entity`.
const SIGNATURES: &[(&str, &str)] = &[
    ("(&mut GameWorld,EntityKey)", "ThinkFn"),
    ("(&mut GameWorld,EntityKey)->bool", "MonsterCheckAttackFn"),
    ("(&mut GameWorld,EntityKey,EntityKey)", "BlockedFn"),
    ("(&mut GameWorld,EntityKey,EntityKey,EntityKey)", "UseFn"),
    ("(&mut GameWorld,EntityKey,EntityKey,f32)", "MonsterDodgeFn"),
    ("(&mut GameWorld,EntityKey,EntityKey,f32,i32)", "PainFn"),
    (
        "(&mut GameWorld,EntityKey,EntityKey,Option<&Plane>,Option<&Surface>)",
        "TouchFn",
    ),
    (
        "(&mut GameWorld,EntityKey,EntityKey,EntityKey,i32,Vec3f)",
        "DieFn",
    ),
];

struct Found {
    /// (callback type, crate-relative path).
    fns: Vec<(&'static str, String)>,
    moves: Vec<String>,
    /// Identifiers used as a value rather than called.
    values: BTreeSet<String>,
}

fn main() {
    let src =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets it")).join("src");
    println!("cargo:rerun-if-changed=src");
    let mut found = Found {
        fns: Vec::new(),
        moves: Vec::new(),
        values: BTreeSet::new(),
    };
    scan_dir(&src, &src, &mut found);

    let mut tables: BTreeMap<&str, BTreeSet<String>> = SIGNATURES
        .iter()
        .map(|&(_, ty)| (ty, BTreeSet::new()))
        .collect();
    for (ty, path) in found.fns {
        let name = path.rsplit("::").next().unwrap_or(&path);
        if found.values.contains(name) {
            tables.get_mut(ty).expect("listed").insert(path);
        }
    }

    let mut out = String::new();
    for (ty, paths) in &tables {
        let _ = writeln!(
            out,
            "pub(super) static {}: &[(&str, {ty})] = &[",
            table_name(ty)
        );
        for path in paths {
            let _ = writeln!(out, "    ({path:?}, crate::{path} as {ty}),");
        }
        out.push_str("];\n");
    }
    found.moves.sort();
    out.push_str("pub(super) static MONSTER_MOVES: &[(&str, &MonsterMove)] = &[\n");
    for path in &found.moves {
        let _ = writeln!(out, "    ({path:?}, &crate::{path}),");
    }
    out.push_str("];\n");

    let dest =
        PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets it")).join("save_tables.rs");
    std::fs::write(dest, out).expect("write save_tables.rs");
}

/// `ThinkFn` -> `THINK_FN`.
fn table_name(ty: &str) -> String {
    let mut name = String::new();
    for (i, c) in ty.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            name.push('_');
        }
        name.push(c.to_ascii_uppercase());
    }
    name
}

fn scan_dir(root: &Path, dir: &Path, found: &mut Found) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .expect("read src")
        .flatten()
        .map(|e| e.path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            scan_dir(root, &path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let text = std::fs::read_to_string(&path).expect("read source");
            let file = syn::parse_file(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            scan_items(&module_path(root, &path), &file.items, found);
        }
    }
}

/// `src/monster/soldier.rs` -> `monster::soldier`; `lib.rs` and `mod.rs`
/// name their directory.
fn module_path(root: &Path, file: &Path) -> String {
    let rel = file
        .strip_prefix(root)
        .expect("under src")
        .with_extension("");
    let mut parts: Vec<String> = rel
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    if matches!(parts.last().map(String::as_str), Some("mod" | "lib")) {
        parts.pop();
    }
    parts.join("::")
}

fn join(module: &str, name: &str) -> String {
    match module {
        "" => name.to_string(),
        _ => format!("{module}::{name}"),
    }
}

fn is_test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg")
            && attr
                .meta
                .to_token_stream()
                .to_string()
                .replace(' ', "")
                .contains("test")
    })
}

fn scan_items(module: &str, items: &[syn::Item], found: &mut Found) {
    for item in items {
        match item {
            syn::Item::Mod(m) if !is_test_only(&m.attrs) => {
                if let Some((_, inner)) = &m.content {
                    scan_items(&join(module, &m.ident.to_string()), inner, found);
                }
            }
            syn::Item::Fn(f) if !is_test_only(&f.attrs) => {
                if let Some(ty) = callback_type(&f.sig) {
                    found.fns.push((ty, join(module, &f.sig.ident.to_string())));
                }
                ValueUses(&mut found.values).visit_item_fn(f);
            }
            syn::Item::Static(s) if !is_test_only(&s.attrs) => {
                if s.ty.to_token_stream().to_string() == "MonsterMove" {
                    found.moves.push(join(module, &s.ident.to_string()));
                }
                ValueUses(&mut found.values).visit_item_static(s);
            }
            syn::Item::Impl(i) if !is_test_only(&i.attrs) => {
                ValueUses(&mut found.values).visit_item_impl(i)
            }
            syn::Item::Const(c) if !is_test_only(&c.attrs) => {
                ValueUses(&mut found.values).visit_item_const(c)
            }
            _ => {}
        }
    }
}

/// The callback type `sig` has, if any. Paths and lifetimes are dropped so
/// `crate::world::GameWorld` and `Option<&'a Plane>` compare equal to the
/// alias's spelling.
fn callback_type(sig: &syn::Signature) -> Option<&'static str> {
    if !sig.generics.params.is_empty() || sig.asyncness.is_some() || sig.unsafety.is_some() {
        return None;
    }
    let mut args = Vec::new();
    for input in &sig.inputs {
        match input {
            syn::FnArg::Typed(t) => args.push(normalize(&t.ty.to_token_stream().to_string())),
            syn::FnArg::Receiver(_) => return None,
        }
    }
    let mut shape = format!("({})", args.join(","));
    if let syn::ReturnType::Type(_, ty) = &sig.output {
        shape.push_str("->");
        shape.push_str(&normalize(&ty.to_token_stream().to_string()));
    }
    SIGNATURES
        .iter()
        .find(|(s, _)| *s == shape)
        .map(|&(_, ty)| ty)
}

fn normalize(tokens: &str) -> String {
    let mut out = String::new();
    for word in tokens.split_whitespace() {
        if word.starts_with('\'') {
            continue;
        }
        out.push_str(word);
        if word == "mut" {
            out.push(' ');
        }
    }
    // Drop module prefixes: `crate::world::GameWorld` -> `GameWorld`.
    while let Some(pos) = out.find("::") {
        let start = out[..pos]
            .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .map_or(0, |i| i + 1);
        out.replace_range(start..pos + 2, "");
    }
    out
}

/// Collects identifiers used as values: paths that are not the callee of a
/// call.
struct ValueUses<'a>(&'a mut BTreeSet<String>);

impl<'ast> Visit<'ast> for ValueUses<'_> {
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if !matches!(&*call.func, syn::Expr::Path(_)) {
            self.visit_expr(&call.func);
        }
        for arg in &call.args {
            self.visit_expr(arg);
        }
    }

    fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
        if let Some(last) = path.path.segments.last() {
            self.0.insert(last.ident.to_string());
        }
    }

    fn visit_item_mod(&mut self, m: &'ast syn::ItemMod) {
        if !is_test_only(&m.attrs) {
            syn::visit::visit_item_mod(self, m);
        }
    }
}
