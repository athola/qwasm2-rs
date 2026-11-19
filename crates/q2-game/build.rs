// Generates the save-file name tables (C `savegame/tables/gamefunc_list.h`
// and `gamemmove_list.h`, which C checks in and edits by hand).
//
// A function is registered when its signature is one of the entity
// callback types and the crate uses it as a value somewhere (assigns or
// passes it rather than calls it). Every `MonsterMove` static is
// registered. The generated paths must be visible from the crate root, so
// a private callback fails to compile until it is made `pub(crate)`.
//
// Plain comments, not `//!`: `tests/build_script.rs` includes this file.

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

#[derive(Default)]
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
    let mut found = Found::default();
    scan_file(&src.join("lib.rs"), "", &mut found);

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

/// Scan the module in `file` and every non-test module it declares, as
/// rustc would: `#[cfg(test)] mod tests;` leaves its file out.
fn scan_file(file: &Path, module: &str, found: &mut Found) {
    let text = std::fs::read_to_string(file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
    let parsed = syn::parse_file(&text).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
    // `lib.rs` and `mod.rs` hold their children beside them; `foo.rs` in `foo/`.
    let dir = match file.file_stem().and_then(|s| s.to_str()) {
        Some("lib" | "mod") => file.parent().expect("in src").to_path_buf(),
        Some(stem) => file.with_file_name(stem),
        None => unreachable!("source files have names"),
    };
    scan_items(module, &parsed.items, found);
    for item in &parsed.items {
        if let syn::Item::Mod(m) = item {
            if m.content.is_none() && !is_test_only(&m.attrs) {
                let name = m.ident.to_string();
                let flat = dir.join(format!("{name}.rs"));
                let nested = dir.join(&name).join("mod.rs");
                let child = if flat.exists() { flat } else { nested };
                scan_file(&child, &join(module, &name), found);
            }
        }
    }
}

fn join(module: &str, name: &str) -> String {
    match module {
        "" => name.to_string(),
        _ => format!("{module}::{name}"),
    }
}

fn is_test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg") && attr.parse_args().is_ok_and(|pred| needs_test(&pred))
    })
}

/// Whether cfg predicate `pred` holds only under `cfg(test)`: `test`, or an
/// `all(..)` with such a term. `not(test)` or `any(test, ..)` may hold
/// without it.
fn needs_test(pred: &syn::Meta) -> bool {
    match pred {
        syn::Meta::Path(path) => path.is_ident("test"),
        syn::Meta::List(list) if list.path.is_ident("all") => list
            .parse_args_with(
                syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
            )
            .is_ok_and(|terms| terms.iter().any(needs_test)),
        _ => false,
    }
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
            _ => {}
        }
    }
}

/// The callback type `sig` has, if any. Paths and lifetimes are dropped so
/// `crate::world::GameWorld` and `Option<&'a Plane>` compare equal to the
/// alias's spelling.
fn callback_type(sig: &syn::Signature) -> Option<&'static str> {
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
}

/// Run by `tests/build_script.rs`, which includes this file: a build
/// script's own tests are never compiled.
#[cfg(test)]
mod tests {
    use super::*;

    fn scan(source: &str) -> Found {
        let file = syn::parse_file(source).expect("test source parses");
        let mut found = Found::default();
        scan_items("m", &file.items, &mut found);
        found
    }

    fn callbacks(found: &Found) -> Vec<&str> {
        found.fns.iter().map(|(_, path)| path.as_str()).collect()
    }

    const THINK: &str = "(w: &mut GameWorld, k: EntityKey)";

    #[test]
    fn only_a_test_or_all_test_cfg_leaves_an_item_out() {
        let source = [
            ("#[cfg(test)]", "a"),
            ("#[cfg(all(test, feature = \"x\"))]", "b"),
            ("#[cfg(not(test))]", "c"),
            ("#[cfg(feature = \"testing\")]", "d"),
            ("#[cfg(any(test, debug_assertions))]", "e"),
            ("", "f"),
        ]
        .map(|(cfg, name)| format!("{cfg} fn {name}{THINK} {{}}\n"))
        .concat();
        assert_eq!(callbacks(&scan(&source)), ["m::c", "m::d", "m::e", "m::f"]);
    }

    #[test]
    fn a_not_test_module_is_scanned() {
        let source = format!(
            "#[cfg(test)] mod t {{ fn a{THINK} {{}} }}\n#[cfg(not(test))] mod n {{ fn b{THINK} {{}} }}"
        );
        assert_eq!(callbacks(&scan(&source)), ["m::n::b"]);
    }
}
