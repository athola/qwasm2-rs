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

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// Crate directory name -> workspace crates named in any of its dependency
/// tables (normal, dev, build, and `[target.'cfg(..)'.dependencies]`).
fn workspace_graph() -> BTreeMap<String, BTreeSet<String>> {
    let mut graph = BTreeMap::new();
    for entry in fs::read_dir(crates_dir()).unwrap() {
        let dir = entry.unwrap().path();
        let manifest = dir.join("Cargo.toml");
        if manifest.is_file() {
            let name = dir.file_name().unwrap().to_str().unwrap().to_owned();
            graph.insert(name, manifest_deps(&fs::read_to_string(manifest).unwrap()));
        }
    }
    graph
}

fn manifest_deps(toml: &str) -> BTreeSet<String> {
    let mut deps = BTreeSet::new();
    let mut in_deps = false;
    for line in toml.lines().map(str::trim) {
        if let Some(header) = line.strip_prefix('[') {
            let header = header.trim_end_matches(']');
            // `[dependencies.q2-x]` names the dependency in the header.
            if let Some((table, dep)) = header.rsplit_once('.') {
                if table.ends_with("dependencies") {
                    deps.insert(dep.to_owned());
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
        deps.insert(name.trim().to_owned());
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
"#;
    assert_eq!(
        manifest_deps(toml),
        ["q2-common", "q2-game", "q2-server", "q2-shared"]
            .map(String::from)
            .into()
    );
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
            // Escaped char literal: '\n', '\'', '\u{..}'.
            let len = chars[i + 2..].iter().position(|&q| q == '\'').unwrap() + 3;
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

/// `// SAFETY:` in the run of comment lines directly above `line` (1-based),
/// or on the line itself.
fn has_safety_comment(src: &str, line: usize) -> bool {
    let lines: Vec<&str> = src.lines().collect();
    if lines[line - 1].contains("// SAFETY:") {
        return true;
    }
    lines[..line - 1]
        .iter()
        .rev()
        .map(|l| l.trim())
        .take_while(|l| l.starts_with("//"))
        .any(|l| l.starts_with("// SAFETY:"))
}

/// Every `.rs` file under `crates/`, with its crate directory name.
fn shown(path: &Path) -> String {
    let rel = path.strip_prefix(crates_dir()).unwrap_or(path);
    format!("crates/{}", rel.display())
}

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
    for krate in workspace_graph().keys() {
        walk(&crates_dir().join(krate), krate, &mut out);
    }
    out
}

fn unsafe_violations(krate: &str, src: &str) -> Vec<(usize, &'static str)> {
    word_lines(&code_only(src), "unsafe")
        .into_iter()
        .filter_map(|line| {
            if krate != UNSAFE_CRATE {
                Some((line, "unsafe outside q2-render"))
            } else if !has_safety_comment(src, line) {
                Some((line, "unsafe without a `// SAFETY:` comment above it"))
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn unsafe_only_in_q2_render_and_always_with_a_safety_comment() {
    let mut broken = Vec::new();
    for (krate, path) in rust_sources() {
        let src = fs::read_to_string(&path).unwrap();
        for (line, why) in unsafe_violations(&krate, &src) {
            broken.push(format!("{}:{line}: {why}", shown(&path)));
        }
    }
    assert!(
        broken.is_empty(),
        "unsafe rule broken (CLAUDE.md Rules, ADR-001 \"Unsafe\", {ADR_001}):\n  {}",
        broken.join("\n  ")
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
#![deny(unsafe_code)]
"##;
    assert!(unsafe_violations("q2-game", src).is_empty());
}

#[test]
fn unsafe_scan_requires_safety_directly_above() {
    let ok = "// SAFETY: context is live.\n// More detail.\nunsafe { f() };\n";
    assert!(unsafe_violations(UNSAFE_CRATE, ok).is_empty());
    let gap = "// SAFETY: context is live.\nlet x = 1;\nunsafe { f() };\n";
    assert_eq!(
        unsafe_violations(UNSAFE_CRATE, gap),
        [(3, "unsafe without a `// SAFETY:` comment above it")]
    );
    assert_eq!(
        unsafe_violations("q2-game", ok),
        [(3, "unsafe outside q2-render")]
    );
}
