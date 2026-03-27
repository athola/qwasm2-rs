//! q2-bundler: Produces a single self-contained HTML file with WASM inlined as base64.
//!
//! Usage:
//!   wasm-pack build --target web --release crates/q2-wasm
//!   cargo run -p q2-bundler
//!
//! Output: dist/qwasm2.html (single file, works when opened or served)

use anyhow::{Context, Result};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use std::fs;
use std::path::Path;

fn main() -> Result<()> {
    let pkg_dir = Path::new("crates/q2-wasm/pkg");
    let dist_dir = Path::new("dist");

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
    let js_glue = fs::read_to_string(&js_path)
        .with_context(|| format!("reading {}", js_path.display()))?;
    let wasm_bytes = fs::read(&wasm_path)
        .with_context(|| format!("reading {}", wasm_path.display()))?;

    let wasm_size = wasm_bytes.len();
    let wasm_base64 = BASE64.encode(&wasm_bytes);

    println!("WASM binary: {} bytes ({:.1} KB)", wasm_size, wasm_size as f64 / 1024.0);
    println!("Base64 encoded: {} bytes ({:.1} KB)", wasm_base64.len(), wasm_base64.len() as f64 / 1024.0);

    // Patch the JS glue to load WASM from base64 instead of fetch
    // The wasm-pack generated JS has a function like:
    //   async function __wbg_load(module, imports) { ... }
    // and an init function that calls fetch(). We replace the fetch with base64 decode.
    let patched_js = patch_js_glue(&js_glue, &wasm_base64);

    // Generate single HTML
    let html = generate_html(&patched_js);

    // Write output
    fs::create_dir_all(dist_dir)?;
    let output_path = dist_dir.join("qwasm2.html");
    fs::write(&output_path, &html)?;

    let html_size = html.len();
    println!("Output: {} ({:.1} KB)", output_path.display(), html_size as f64 / 1024.0);
    println!("Done! Open dist/qwasm2.html in a browser.");

    Ok(())
}

/// Patch the wasm-pack JS glue to load WASM from an inlined base64 string
/// instead of fetching a separate .wasm file.
fn patch_js_glue(js: &str, wasm_base64: &str) -> String {
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

    // Strip `export` keywords — can't use them in inline <script type="module">
    // "export function foo()" → "function foo()"
    // "export { initSync, __wbg_init as default };" → "const init = __wbg_init;"
    //
    // NOTE: This export stripping is tightly coupled to wasm-pack's current JS
    // output format. If wasm-pack changes its codegen, this will silently break.
    // Consider using a regex or AST-based approach for robustness.
    let stripped = js
        .replace("export function ", "function ")
        .replace("export async function ", "async function ");

    // Handle the final export line: "export { initSync, __wbg_init as default };"
    // We need `init` to be available as a variable name in the inline script
    let stripped = if stripped.contains("__wbg_init as default") {
        stripped
            .replace(
                "export { initSync, __wbg_init as default };",
                "const init = __wbg_init;"
            )
            // Also handle variations in the export syntax
            .replace(
                "export { initSync }",
                ""
            )
    } else {
        stripped
    };

    // Replace the fetch call to use our inlined WASM
    let patched = if stripped.contains("import.meta.url") {
        stripped.replace(
            "new URL('q2_wasm_bg.wasm', import.meta.url)",
            "__q2_decode_wasm()"
        ).replace(
            r#"new URL("q2_wasm_bg.wasm", import.meta.url)"#,
            "__q2_decode_wasm()"
        )
    } else {
        stripped
    };

    format!("{}{}", decoder, patched)
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
  #click-prompt {{
    position: absolute; top: 50%; left: 50%;
    transform: translate(-50%, -50%);
    font-size: 18px; color: #e94560;
    z-index: 20; text-align: center;
  }}
  #click-prompt.hidden {{ display: none; }}
</style>
</head>
<body>
<div id="game-container">
  <canvas id="canvas"></canvas>
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

  // Determine pak URL — if served by devserver, fetch from /gamedata/
  const pakUrl = '/gamedata/baseq2/pak0.pak';

  await start_game('canvas', pakUrl);
}} catch (e) {{
  const statusEl = document.getElementById('status');
  statusEl.innerHTML += '<span class="fail">ERROR: ' + e + '</span>\n';
  console.error('[qwasm2-rs]', e);
}}
</script>

<!-- Playwright test hooks -->
<div id="test-hooks" style="display:none">
  <span id="wasm-loaded">false</span>
  <span id="self-test-result"></span>
</div>
</body>
</html>"#
    )
}
