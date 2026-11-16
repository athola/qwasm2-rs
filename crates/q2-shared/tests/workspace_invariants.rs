//! Workspace architecture rules from CLAUDE.md and ADR-001, checked against
//! the source tree so a violation fails `cargo test` instead of waiting for a
//! manual `/arch-check`. Std only: this reads files, it does not build them.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const ADR_001: &str = "docs/adr/001-crate-decomposition-and-trait-boundaries.md";

/// ADR-001 "Dependency DAG": every workspace edge each crate may have.
const ALLOWED_DEPS: &[(&str, &[&str])] = &[
    ("q2-shared", &[]),
    ("q2-common", &["q2-shared"]),
    ("q2-render-api", &["q2-shared"]),
    ("q2-game", &["q2-shared", "q2-common"]),
    ("q2-client", &["q2-shared", "q2-common"]),
    ("q2-render", &["q2-shared", "q2-common", "q2-render-api"]),
    ("q2-server", &["q2-shared", "q2-common", "q2-game"]),
    ("q2-platform", &["q2-common"]),
    (
        "q2-wasm",
        &[
            "q2-shared",
            "q2-common",
            "q2-server",
            "q2-client",
            "q2-render-api",
            "q2-render",
            "q2-platform",
        ],
    ),
    ("q2-bin", &[]),
    ("q2-net", &[]),
    ("q2-devserver", &[]),
    ("q2-bundler", &[]),
    ("q2-pak-repack", &[]),
];

/// The only crate allowed to contain `unsafe` (CLAUDE.md Rules, ADR-001).
const UNSAFE_CRATE: &str = "q2-render";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Member paths in the root Cargo.toml `[workspace] members`, the crates
/// cargo builds. Panics on a glob rather than skip the crates it names.
fn workspace_members(toml: &str) -> Vec<String> {
    let at = toml
        .find("\nmembers")
        .expect("root Cargo.toml has no `members`");
    let list = &toml[at..];
    let list = &list[list.find('[').unwrap() + 1..list.find(']').unwrap()];
    let members: Vec<String> = list
        .lines()
        .flat_map(|line| line.split('#').next().unwrap().split(','))
        .map(|m| m.trim().trim_matches('"').to_owned())
        .filter(|m| !m.is_empty())
        .collect();
    assert!(!members.is_empty(), "no workspace members read");
    for m in &members {
        assert!(!m.contains('*'), "member glob {m:?}: list members by path");
    }
    members
}

/// Every workspace member as (crate directory name, directory).
fn member_dirs() -> Vec<(String, PathBuf)> {
    let root = fs::read_to_string(repo_root().join("Cargo.toml")).unwrap();
    workspace_members(&root)
        .into_iter()
        .map(|m| {
            (
                m.rsplit('/').next().unwrap().to_owned(),
                repo_root().join(m),
            )
        })
        .collect()
}

/// Crate directory name -> workspace crates named in any of its dependency
/// tables (normal, dev, build, and `[target.'cfg(..)'.dependencies]`).
fn workspace_graph() -> BTreeMap<String, BTreeSet<String>> {
    member_dirs()
        .into_iter()
        .map(|(name, dir)| {
            let manifest = fs::read_to_string(dir.join("Cargo.toml")).unwrap();
            (name, manifest_deps(&manifest))
        })
        .collect()
}

fn manifest_deps(toml: &str) -> BTreeSet<String> {
    let mut deps = BTreeSet::new();
    let mut in_deps = false;
    for line in toml.lines().map(str::trim) {
        if let Some(header) = line.strip_prefix('[') {
            // `[ dependencies ] # note` is the same table as `[dependencies]`.
            let header = header.split('#').next().unwrap().trim();
            let header = header.trim_end_matches(']').trim();
            // `[dependencies.q2-x]` names the dependency in the header.
            if let Some((table, dep)) = header.rsplit_once('.') {
                if table.trim_end().ends_with("dependencies") {
                    deps.insert(dep.trim().trim_matches('"').to_owned());
                    in_deps = false;
                    continue;
                }
            }
            in_deps = header.ends_with("dependencies");
            continue;
        }
        if !in_deps || line.starts_with('#') {
            continue;
        }
        let Some((key, rest)) = line.split_once('=') else {
            continue;
        };
        // `alias = { package = "q2-x", .. }` depends on q2-x.
        let name = rest
            .split_once("package")
            .and_then(|(_, p)| p.split('"').nth(1))
            .unwrap_or_else(|| key.trim().split('.').next().unwrap());
        // A quoted key, `"q2-x" = ..`, names q2-x.
        deps.insert(name.trim().trim_matches('"').to_owned());
    }
    deps.retain(|d| d.starts_with("q2-"));
    deps
}

/// One cycle in `graph`, as the path that closes it, if any.
fn find_cycle(graph: &BTreeMap<String, BTreeSet<String>>) -> Option<Vec<String>> {
    fn visit(
        node: &str,
        graph: &BTreeMap<String, BTreeSet<String>>,
        path: &mut Vec<String>,
        done: &mut BTreeSet<String>,
    ) -> Option<Vec<String>> {
        if let Some(start) = path.iter().position(|n| n == node) {
            let mut cycle = path[start..].to_vec();
            cycle.push(node.to_owned());
            return Some(cycle);
        }
        if done.contains(node) {
            return None;
        }
        path.push(node.to_owned());
        for dep in graph.get(node).into_iter().flatten() {
            if let Some(cycle) = visit(dep, graph, path, done) {
                return Some(cycle);
            }
        }
        path.pop();
        done.insert(node.to_owned());
        None
    }
    let mut done = BTreeSet::new();
    graph
        .keys()
        .find_map(|n| visit(n, graph, &mut Vec::new(), &mut done))
}

#[test]
fn workspace_crates_are_the_ones_adr_001_lists() {
    let actual: BTreeSet<_> = workspace_graph().into_keys().collect();
    let allowed: BTreeSet<_> = ALLOWED_DEPS.iter().map(|(c, _)| c.to_string()).collect();
    assert_eq!(
        actual, allowed,
        "crate set differs from ADR-001 \"Crate Structure\" ({ADR_001}); \
         update the ADR and ALLOWED_DEPS together"
    );
}

#[test]
fn crate_dependencies_match_the_adr_001_dag() {
    let graph = workspace_graph();
    let mut broken = Vec::new();
    for (krate, allowed) in ALLOWED_DEPS {
        let allowed: BTreeSet<String> = allowed.iter().map(|d| d.to_string()).collect();
        let Some(actual) = graph.get(*krate) else {
            continue;
        };
        for dep in actual.difference(&allowed) {
            broken.push(format!("{krate} depends on {dep}, which ADR-001 forbids"));
        }
        for dep in allowed.difference(actual) {
            broken.push(format!(
                "{krate} no longer depends on {dep}, but ADR-001 lists it"
            ));
        }
    }
    assert!(
        broken.is_empty(),
        "dependency DAG rule broken (ADR-001 \"Dependency DAG\", {ADR_001}):\n  {}\n\
         Remove the edge, or amend the ADR and ALLOWED_DEPS together.",
        broken.join("\n  ")
    );
}

#[test]
fn crate_dependency_graph_has_no_cycles() {
    let cycle = find_cycle(&workspace_graph());
    assert!(
        cycle.is_none(),
        "circular crate dependency {} breaks ADR-001 \"no circular dependencies\" ({ADR_001})",
        cycle.unwrap_or_default().join(" -> ")
    );
}

#[test]
fn cycle_finder_reports_a_cycle_and_accepts_a_dag() {
    let graph = |edges: &[(&str, &str)]| {
        let mut g: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for (from, to) in edges {
            g.entry(from.to_string())
                .or_default()
                .insert(to.to_string());
        }
        g
    };
    let dag = graph(&[("a", "b"), ("a", "c"), ("b", "c")]);
    assert_eq!(find_cycle(&dag), None);
    let cyclic = graph(&[("a", "b"), ("b", "c"), ("c", "a")]);
    assert_eq!(
        find_cycle(&cyclic).unwrap(),
        ["a", "b", "c", "a"].map(String::from)
    );
}

#[test]
fn manifest_deps_reads_every_dependency_table_form() {
    let toml = r#"
[dependencies]
q2-shared = { path = "../q2-shared" }
serde = "1"
[target.'cfg(target_arch = "wasm32")'.dependencies]
q2-common.workspace = true
[dev-dependencies]
game = { package = "q2-game", path = "../q2-game" }
[dependencies.q2-server]
path = "../q2-server"
[lints]
q2-render = "not a dependency"
[build-dependencies] # engine crates
q2-client = { path = "../q2-client" }
[ dependencies ]
"q2-net" = { path = "../q2-net" }
"#;
    assert_eq!(
        manifest_deps(toml),
        [
            "q2-client",
            "q2-common",
            "q2-game",
            "q2-net",
            "q2-server",
            "q2-shared"
        ]
        .map(String::from)
        .into()
    );
}

#[test]
fn workspace_members_reads_every_listed_path_inside_or_outside_crates() {
    let toml = "[workspace]\nresolver = \"2\"\nmembers = [\n    \"crates/q2-shared\", # types\n    \"tools/q2-foo\",\n]\n";
    assert_eq!(
        workspace_members(toml),
        ["crates/q2-shared", "tools/q2-foo"]
    );
}

#[test]
#[should_panic(expected = "glob")]
fn workspace_members_rejects_a_glob_it_cannot_expand() {
    workspace_members("[workspace]\nmembers = [\"crates/*\"]\n");
}

/// `src` with comments, string and char literals blanked to spaces (newlines
/// kept), so keyword searches see only code and line numbers still match.
fn code_only(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let blank = |out: &mut String, c: char| out.push(if c == '\n' { '\n' } else { ' ' });
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        let ident_before = i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_');
        if c == '/' && next == Some('/') {
            while i < chars.len() && chars[i] != '\n' {
                blank(&mut out, chars[i]);
                i += 1;
            }
        } else if c == '/' && next == Some('*') {
            let mut depth = 0;
            loop {
                if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                    depth += 1;
                } else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    depth -= 1;
                }
                if depth == 0 {
                    out.push_str("  ");
                    i += 2;
                    break;
                }
                blank(&mut out, chars[i]);
                i += 1;
            }
        } else if c == 'r' && !ident_before && matches!(next, Some('"' | '#')) {
            // Raw string: r"..." or r#"..."#.
            let hashes = chars[i + 1..].iter().take_while(|&&h| h == '#').count();
            if chars.get(i + 1 + hashes) != Some(&'"') {
                out.push(c);
                i += 1;
                continue;
            }
            let close: String = std::iter::once('"')
                .chain("#".repeat(hashes).chars())
                .collect();
            let body_start = i + 2 + hashes;
            let rest: String = chars[body_start..].iter().collect();
            let end = body_start + rest.find(&close).map_or(rest.len(), |e| e + close.len());
            for &ch in &chars[i..end.min(chars.len())] {
                blank(&mut out, ch);
            }
            i = end;
        } else if c == '"' {
            blank(&mut out, c);
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                let step = if chars[i] == '\\' { 2 } else { 1 };
                for &ch in &chars[i..(i + step).min(chars.len())] {
                    blank(&mut out, ch);
                }
                i += step;
            }
            blank(&mut out, '"');
            i += 1;
        } else if c == '\'' && next == Some('\\') {
            // Escaped char literal: '\n', '\'', '\\', '\u{..}'. The closing
            // quote is the first one after the escaped character.
            let len = chars[i + 3..].iter().position(|&q| q == '\'').unwrap() + 4;
            out.push_str(&" ".repeat(len));
            i += len;
        } else if c == '\'' && chars.get(i + 2) == Some(&'\'') {
            out.push_str("   ");
            i += 3;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// 1-based line numbers where `word` appears as a whole identifier.
fn word_lines(code: &str, word: &str) -> Vec<usize> {
    let is_ident = |c: char| c.is_alphanumeric() || c == '_';
    let mut lines = Vec::new();
    for (n, line) in code.lines().enumerate() {
        let mut from = 0;
        while let Some(at) = line[from..].find(word) {
            let start = from + at;
            let end = start + word.len();
            let before = line[..start].chars().next_back();
            let after = line[end..].chars().next();
            if !before.is_some_and(is_ident) && !after.is_some_and(is_ident) {
                lines.push(n + 1);
            }
            from = end;
        }
    }
    lines
}

/// `path` relative to the repo root, for failure messages.
fn shown(path: &Path) -> String {
    let rel = path.strip_prefix(repo_root()).unwrap_or(path);
    rel.display().to_string()
}

/// Every `.rs` file in a workspace member, with its crate directory name.
fn rust_sources() -> Vec<(String, PathBuf)> {
    fn walk(dir: &Path, krate: &str, out: &mut Vec<(String, PathBuf)>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_str().unwrap();
            if path.is_dir() && name != "target" && name != "pkg" {
                walk(&path, krate, out);
            } else if name.ends_with(".rs") {
                out.push((krate.to_owned(), path));
            }
        }
    }
    let mut out = Vec::new();
    for (krate, dir) in member_dirs() {
        walk(&dir, &krate, &mut out);
    }
    out
}

/// 1-based lines using `unsafe` in a crate other than q2-render. Whether a
/// q2-render block has its `// SAFETY:` comment is clippy's
/// `undocumented_unsafe_blocks` (denied in the workspace Cargo.toml).
fn unsafe_violations(krate: &str, src: &str) -> Vec<usize> {
    if krate == UNSAFE_CRATE {
        return Vec::new();
    }
    word_lines(&code_only(src), "unsafe")
}

#[test]
fn unsafe_only_in_q2_render() {
    let mut broken = Vec::new();
    for (krate, path) in rust_sources() {
        let src = fs::read_to_string(&path).unwrap();
        for line in unsafe_violations(&krate, &src) {
            broken.push(format!("{}:{line}", shown(&path)));
        }
    }
    assert!(
        broken.is_empty(),
        "unsafe outside q2-render (CLAUDE.md Rules, ADR-001 \"Unsafe\", {ADR_001}):\n  {}",
        broken.join("\n  ")
    );
}

/// ADR-001 points readers to the unsafe audit in q2-render's lib.rs header,
/// so its "Total unsafe blocks: N" must match the `unsafe` tokens in the code.
#[test]
fn q2_render_header_states_its_unsafe_block_count() {
    let counted: usize = rust_sources()
        .into_iter()
        .filter(|(krate, _)| krate == UNSAFE_CRATE)
        .map(|(_, path)| word_lines(&code_only(&fs::read_to_string(path).unwrap()), "unsafe").len())
        .sum();
    let (_, dir) = member_dirs()
        .into_iter()
        .find(|(krate, _)| krate == UNSAFE_CRATE)
        .expect("q2-render is a workspace member");
    let lib = dir.join("src/lib.rs");
    let stated: usize = fs::read_to_string(&lib)
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix("//! - Total unsafe blocks: "))
        .and_then(|rest| rest.split_whitespace().next()?.parse().ok())
        .expect("q2-render lib.rs states \"Total unsafe blocks: N\"");
    assert_eq!(
        counted,
        stated,
        "{} states {stated} unsafe blocks; the code has {counted}",
        shown(&lib)
    );
}

/// 1-based lines declaring a `static mut`, comments and strings ignored.
fn static_mut_lines(src: &str) -> Vec<usize> {
    code_only(src)
        .lines()
        .enumerate()
        .filter(|(_, line)| {
            let words: Vec<&str> = line
                .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                .filter(|w| !w.is_empty())
                .collect();
            words.windows(2).any(|w| w == ["static", "mut"])
        })
        .map(|(n, _)| n + 1)
        .collect()
}

#[test]
fn static_mut_scan_finds_declarations_but_not_mentions() {
    let src = "// static mut in a comment\nconst S: &str = \"static mut\";\nstatic  mut COUNT: u32 = 0;\nstatic NAME: &str = \"\";\n";
    assert_eq!(static_mut_lines(src), [3]);
}

#[test]
fn no_static_mut_anywhere() {
    let mut broken = Vec::new();
    for (_, path) in rust_sources() {
        let src = fs::read_to_string(&path).unwrap();
        for line in static_mut_lines(&src) {
            broken.push(format!("{}:{line}", shown(&path)));
        }
    }
    assert!(
        broken.is_empty(),
        "`static mut` breaks the no-global-mutable-state rule (CLAUDE.md Conventions, \
         ADR-001, {ADR_001}):\n  {}",
        broken.join("\n  ")
    );
}

#[test]
fn unsafe_scan_ignores_comments_strings_and_lint_names() {
    let src = r##"
// unsafe in a comment
/* unsafe /* nested */ unsafe */
let s = "unsafe { }";
let r = r#"unsafe "quoted" { }"#;
let c = '"';
let q = '\''; let s = "unsafe";
let b = '\\'; let s = "unsafe";
#![deny(unsafe_code)]
"##;
    assert!(unsafe_violations("q2-game", src).is_empty());
}

#[test]
fn code_only_blanks_a_whole_escaped_char_literal() {
    for lit in [r"'\''", r"'\\'", r"'\n'", r"'\u{7f}'"] {
        let code = code_only(&format!("{lit}x"));
        assert_eq!(code, format!("{}x", " ".repeat(lit.len())), "{lit}");
    }
}

#[test]
fn unsafe_scan_flags_every_crate_but_q2_render() {
    let src = "// SAFETY: context is live.\nunsafe { f() };\n";
    assert!(unsafe_violations(UNSAFE_CRATE, src).is_empty());
    assert_eq!(unsafe_violations("q2-game", src), [2]);
}

/// The C this workspace ports (id's Quake II, yquake2, Qwasm2) is
/// GPL-2.0-or-later, and a translation of it is a derivative work, so no
/// crate may declare a licence of its own.
const LICENSE: &str = "GPL-2.0-or-later";

#[test]
fn every_crate_is_gpl_2_or_later_like_the_c_it_ports() {
    let root = fs::read_to_string(repo_root().join("Cargo.toml")).unwrap();
    let package = &root[root
        .find("[workspace.package]")
        .expect("root Cargo.toml has [workspace.package]")..];
    let package = &package[..package[1..].find("\n[").map_or(package.len(), |i| i + 1)];
    assert!(
        package.contains(&format!("license = \"{LICENSE}\"")),
        "[workspace.package] must set license = \"{LICENSE}\""
    );
    for (krate, dir) in member_dirs() {
        let manifest = fs::read_to_string(dir.join("Cargo.toml")).unwrap();
        let lines: Vec<_> = manifest
            .lines()
            .map(str::trim)
            .filter(|l| l.starts_with("license"))
            .collect();
        assert_eq!(
            lines,
            ["license.workspace = true"],
            "{krate} must inherit the workspace licence"
        );
    }
    let text = fs::read_to_string(repo_root().join("LICENSE")).expect("LICENSE at the root");
    assert!(text.contains("GNU GENERAL PUBLIC LICENSE") && text.contains("Version 2, June 1991"));
    // As Qwasm2's LICENSE does, credit the C's authors and carry the notice
    // of stb, whose TGA decoder q2-render's tga.rs follows.
    let notices = fs::read_to_string(repo_root().join("THIRD-PARTY-NOTICES"))
        .expect("THIRD-PARTY-NOTICES at the root");
    for credit in ["Id Software", "Yamagi Quake II", "Qwasm2", "Sean Barrett"] {
        assert!(
            notices.contains(credit),
            "THIRD-PARTY-NOTICES omits {credit}"
        );
    }
}
