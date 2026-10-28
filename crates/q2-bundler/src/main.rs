//! q2-bundler: Produces a single self-contained HTML file with WASM inlined as base64.
//!
//! Usage:
//!   wasm-pack build --target web --release crates/q2-wasm
//!   cargo run -p q2-bundler
//!
//! Output: dist/qwasm2.html (single file; serve it with q2-devserver, since the
//! page fetches `/gamedata/baseq2/pak0-web.pak` from the same origin)

use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use std::fs;
use std::path::Path;

fn main() -> Result<()> {
    // Resolve from the repo root so the bundler works from any directory.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let pkg_dir = root.join("crates/q2-wasm/pkg");
    let dist_dir = root.join("dist");

    // Find the JS glue file
    let js_path = pkg_dir.join("q2_wasm.js");
    let wasm_path = pkg_dir.join("q2_wasm_bg.wasm");

    if !js_path.exists() || !wasm_path.exists() {
        anyhow::bail!(
            "wasm-pack output not found at {}\n\
             Run: wasm-pack build --target web crates/q2-wasm\n\
             Then re-run this bundler.",
            pkg_dir.display()
        );
    }

    // Read files
    let js_glue =
        fs::read_to_string(&js_path).with_context(|| format!("reading {}", js_path.display()))?;
    let wasm_bytes =
        fs::read(&wasm_path).with_context(|| format!("reading {}", wasm_path.display()))?;

    let wasm_size = wasm_bytes.len();
    let wasm_base64 = BASE64.encode(&wasm_bytes);

    println!(
        "WASM binary: {} bytes ({:.1} KB)",
        wasm_size,
        wasm_size as f64 / 1024.0
    );
    println!(
        "Base64 encoded: {} bytes ({:.1} KB)",
        wasm_base64.len(),
        wasm_base64.len() as f64 / 1024.0
    );

    // Patch the JS glue to load WASM from base64 instead of fetch
    // The wasm-pack generated JS has a function like:
    //   async function __wbg_load(module, imports) { ... }
    // and an init function that calls fetch(). We replace the fetch with base64 decode.
    let patched_js = patch_js_glue(&js_glue, &wasm_base64)?;

    // Generate single HTML
    let html = generate_html(&patched_js);

    // Write output
    fs::create_dir_all(&dist_dir)?;
    let output_path = dist_dir.join("qwasm2.html");
    fs::write(&output_path, &html)?;

    let html_size = html.len();
    println!(
        "Output: {} ({:.1} KB)",
        output_path.display(),
        html_size as f64 / 1024.0
    );
    println!("Done! Serve it with `make devserver` (the page fetches the pak from /gamedata/).");

    Ok(())
}

/// Patch the wasm-pack JS glue to load WASM from an inlined base64 string
/// instead of fetching a separate .wasm file.
fn patch_js_glue(js: &str, wasm_base64: &str) -> Result<String> {
    // The wasm-pack --target web output has an `init` function that accepts
    // an optional input (URL or Module). We inject the base64 decode at the top
    // and modify the init to use it.
    //
    // Strategy: prepend a helper that decodes base64 → Uint8Array → WebAssembly.Module,
    // then replace the fetch-based loading with our pre-decoded module.

    let decoder = format!(
        r#"
// --- q2-bundler: inlined WASM binary ---
const __q2_wasm_base64 = "{}";

function __q2_decode_wasm() {{
    const binaryString = atob(__q2_wasm_base64);
    const bytes = new Uint8Array(binaryString.length);
    for (let i = 0; i < binaryString.length; i++) {{
        bytes[i] = binaryString.charCodeAt(i);
    }}
    return bytes.buffer;
}}
// --- end q2-bundler injection ---
"#,
        wasm_base64
    );

    // Inline <script type="module"> code cannot import sibling files, and
    // `init` must be a plain binding for the page script below. Each rewrite
    // is tied to wasm-bindgen's current output, so each must match exactly
    // once: a codegen change fails the build here instead of producing a page
    // that fetches a missing .wasm. (An AST-based rewrite is tracked in #2.)
    if let Some(line) = js.lines().find(|l| l.trim_start().starts_with("import ")) {
        anyhow::bail!("wasm-bindgen glue has a module import the bundler cannot inline: {line}");
    }

    let patched = js
        .replace("export function ", "function ")
        .replace("export async function ", "async function ");
    let patched = replace_once(
        &patched,
        &["export { initSync, __wbg_init as default };"],
        "const init = __wbg_init;",
    )?;
    let patched = replace_once(
        &patched,
        &[
            "new URL('q2_wasm_bg.wasm', import.meta.url)",
            r#"new URL("q2_wasm_bg.wasm", import.meta.url)"#,
        ],
        "__q2_decode_wasm()",
    )?;
    // Exposes the raw wasm exports to importers; nothing imports this page.
    let patched = patched.replace("export { wasm as __wasm }", "");

    ensure!(
        !patched.contains("import.meta.url"),
        "wasm-bindgen glue still references import.meta.url after patching"
    );
    if let Some(line) = patched.lines().find(|l| l.starts_with("export ")) {
        anyhow::bail!("unhandled export in wasm-bindgen glue: {line}");
    }
    // The glue goes verbatim into an inline <script>, where `</script` ends
    // the element and `<!--` can switch the parser into escaped text.
    let lower = patched.to_ascii_lowercase();
    ensure!(
        !lower.contains("</script") && !lower.contains("<!--"),
        "wasm-bindgen glue contains `</script` or `<!--`, which an inline <script> cannot hold"
    );

    Ok(format!("{}{}", decoder, patched))
}

/// Replace the single occurrence of any of `patterns` (alternative spellings
/// of one construct) with `with`; zero or several matches is an error.
fn replace_once(src: &str, patterns: &[&str], with: &str) -> Result<String> {
    let found: Vec<&str> = patterns
        .iter()
        .copied()
        .filter(|p| src.contains(p))
        .collect();
    let count: usize = found.iter().map(|p| src.matches(p).count()).sum();
    ensure!(
        count == 1,
        "expected exactly one `{}` in wasm-bindgen glue, found {count}; \
         the wasm-pack output format changed",
        patterns[0]
    );
    Ok(src.replacen(found[0], with, 1))
}

fn generate_html(inlined_js: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Qwasm2-rs</title>
<style>
  * {{ margin: 0; padding: 0; box-sizing: border-box; }}
  body {{ background: #0a0a0a; color: #e0e0e0; font-family: 'Courier New', monospace; overflow: hidden; }}
  #game-container {{ position: relative; width: 100vw; height: 100vh; }}
  canvas {{
    width: 100%; height: 100%;
    display: block;
    background: #16213e;
    cursor: crosshair;
  }}
  #status-container {{
    position: absolute; bottom: 0; left: 0; right: 0;
    z-index: 10;
  }}
  #status-container.hidden {{ display: none; }}
  #status-toolbar {{
    display: flex; gap: 4px; padding: 4px 8px;
    background: rgba(0, 0, 0, 0.95);
    border-bottom: 1px solid #333;
  }}
  #status-toolbar button {{
    background: #222; color: #aaa; border: 1px solid #444;
    padding: 2px 8px; font-size: 11px; font-family: inherit;
    cursor: pointer;
  }}
  #status-toolbar button:hover {{ background: #444; color: #fff; }}
  #status {{
    padding: 8px 12px;
    background: rgba(0, 0, 0, 0.85);
    white-space: pre-wrap; font-size: 13px;
    max-height: 35vh; overflow-y: auto;
    user-select: text; cursor: text;
  }}
  .pass {{ color: #4ecca3; }}
  .fail {{ color: #e94560; }}
  .info {{ color: #a0a0d0; }}
  #hud {{
    position: absolute; top: 8px; left: 12px;
    font-size: 12px; color: #888; z-index: 10;
    pointer-events: none;
  }}
  #crosshair {{
    position: absolute; top: 50%; left: 50%;
    transform: translate(-50%, -50%);
    width: 14px; height: 14px;
    pointer-events: none; z-index: 15;
  }}
  #crosshair::before, #crosshair::after {{
    content: ''; position: absolute;
    background: rgba(255, 255, 255, 0.9);
    box-shadow: 0 0 2px rgba(0,0,0,0.8);
  }}
  #crosshair::before {{ left: 0; top: calc(50% - 1px); width: 100%; height: 2px; }}
  #crosshair::after  {{ top: 0; left: calc(50% - 1px); width: 2px; height: 100%; }}
  #click-prompt {{
    position: absolute; top: 50%; left: 50%;
    transform: translate(-50%, -50%);
    font-size: 18px; color: #e94560;
    z-index: 20; text-align: center;
    pointer-events: none;
  }}
  #click-prompt.hidden {{ display: none; }}
</style>
</head>
<body>
<div id="game-container">
  <canvas id="canvas"></canvas>
  <div id="crosshair"></div>
  <div id="hud"></div>
  <div id="click-prompt">Click to play<br><span style="font-size:12px;color:#888">WASD move &middot; Mouse look &middot; Space jump &middot; Esc release &middot; ` console</span></div>
  <div id="status-container">
    <div id="status-toolbar">
      <button onclick="copyLog()">Copy to clipboard</button>
      <button onclick="saveLog()">Save to file</button>
      <button onclick="clearLog()">Clear</button>
      <span style="color:#555;font-size:11px;margin-left:auto">` to toggle</span>
    </div>
    <div id="status"><span class="info">Loading engine...</span></div>
  </div>
</div>

<script type="module">
{inlined_js}

// Resize canvas to window
function resize() {{
  const canvas = document.getElementById('canvas');
  canvas.width = window.innerWidth;
  canvas.height = window.innerHeight;
}}
resize();
window.addEventListener('resize', resize);

// Hide click prompt on pointer lock
document.addEventListener('pointerlockchange', () => {{
  const el = document.getElementById('click-prompt');
  if (document.pointerLockElement) {{
    el.classList.add('hidden');
  }} else {{
    el.classList.remove('hidden');
  }}
}});

// Toggle status log with backtick
document.addEventListener('keydown', (e) => {{
  if (e.code === 'Backquote') {{
    document.getElementById('status-container').classList.toggle('hidden');
  }}
}});

// Log utilities
window.copyLog = function() {{
  const text = document.getElementById('status').innerText;
  navigator.clipboard.writeText(text).then(() => {{
    const btn = document.querySelector('#status-toolbar button');
    const orig = btn.textContent;
    btn.textContent = 'Copied!';
    setTimeout(() => btn.textContent = orig, 1000);
  }});
}};

window.saveLog = function() {{
  const text = document.getElementById('status').innerText;
  const blob = new Blob([text], {{ type: 'text/plain' }});
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = 'qwasm2-log-' + new Date().toISOString().slice(0,19).replace(/:/g,'-') + '.txt';
  a.click();
  URL.revokeObjectURL(url);
}};

window.clearLog = function() {{
  document.getElementById('status').innerHTML = '';
}};

// Initialize and start game
try {{
  await init();

  // Populate Playwright test hooks after WASM init succeeds
  document.getElementById('wasm-loaded').textContent = 'true';
  document.getElementById('self-test-result').textContent = self_test();
  document.getElementById('engine-version').textContent = engine_version();

  // Determine pak URL — if served by devserver, fetch from /gamedata/
  const pakUrl = '/gamedata/baseq2/pak0-web.pak';

  await start_game('canvas', pakUrl);
}} catch (e) {{
  // textContent: error text can carry pak-derived strings; never parse it as HTML.
  const line = document.createElement('span');
  line.className = 'fail';
  line.textContent = 'ERROR: ' + e;
  const statusEl = document.getElementById('status');
  statusEl.appendChild(line);
  statusEl.appendChild(document.createTextNode('\n'));
  console.error('[qwasm2-rs]', e);
}}
</script>

<!-- Playwright test hooks -->
<div id="test-hooks" style="display:none">
  <span id="wasm-loaded">false</span>
  <span id="self-test-result"></span>
  <span id="engine-version"></span>
</div>
</body>
</html>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed wasm-pack 0.14.0 / wasm-bindgen 0.2.114 `--target web` output.
    const GLUE: &str = include_str!("../fixtures/wasm_bindgen_glue.js");

    #[test]
    fn patch_inlines_wasm_and_exposes_init() {
        let patched = patch_js_glue(GLUE, "QUJD").unwrap();
        assert!(patched.contains(r#"const __q2_wasm_base64 = "QUJD";"#));
        assert!(patched.contains("module_or_path = __q2_decode_wasm();"));
        assert!(patched.contains("const init = __wbg_init;"));
        assert!(!patched.contains("import.meta.url"));
        assert!(!patched.lines().any(|l| l.starts_with("export ")));
    }

    #[test]
    fn patch_accepts_double_quoted_wasm_url() {
        let glue = GLUE.replace(
            "new URL('q2_wasm_bg.wasm', import.meta.url)",
            r#"new URL("q2_wasm_bg.wasm", import.meta.url)"#,
        );
        assert!(patch_js_glue(&glue, "").is_ok());
    }

    #[test]
    fn patch_fails_when_wasm_url_format_changes() {
        let glue = GLUE.replace(
            "new URL('q2_wasm_bg.wasm', import.meta.url)",
            "new URL('renamed_bg.wasm', import.meta.url)",
        );
        let err = patch_js_glue(&glue, "").unwrap_err().to_string();
        assert!(err.contains("q2_wasm_bg.wasm"), "{err}");
    }

    #[test]
    fn patch_fails_when_default_export_changes() {
        let glue = GLUE.replace(
            "export { initSync, __wbg_init as default };",
            "export { __wbg_init as default, initSync };",
        );
        let err = patch_js_glue(&glue, "").unwrap_err().to_string();
        assert!(err.contains("__wbg_init as default"), "{err}");
    }

    #[test]
    fn patch_fails_when_glue_would_end_or_escape_the_inline_script() {
        for snippet in [
            "const s = '</script>';",
            "const s = '</SCRIPT ';",
            "// <!--",
        ] {
            let glue = format!("{GLUE}\n{snippet}\n");
            let err = patch_js_glue(&glue, "").unwrap_err().to_string();
            assert!(err.contains("inline <script>"), "{snippet}: {err}");
        }
    }

    #[test]
    fn patch_fails_on_module_imports() {
        let glue = format!("import {{ helper }} from './snippets/x.js';\n{GLUE}");
        let err = patch_js_glue(&glue, "").unwrap_err().to_string();
        assert!(err.contains("import"), "{err}");
    }

    #[test]
    fn click_prompt_lets_clicks_through_to_the_canvas() {
        // The prompt sits over the canvas centre; clicks on its text must
        // still reach the canvas to lock the pointer and start audio.
        let html = generate_html("");
        let rule = html
            .split("#click-prompt {")
            .nth(1)
            .and_then(|r| r.split('}').next())
            .expect("click-prompt rule");
        assert!(rule.contains("pointer-events: none"), "{rule}");
    }

    #[test]
    fn page_error_path_does_not_inject_html() {
        let html = generate_html("");
        assert!(
            !html.contains("innerHTML +="),
            "error text must not be parsed as HTML"
        );
        assert!(html.contains("line.textContent = 'ERROR: ' + e;"));
    }
}
