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
  body {{ background: #1a1a2e; color: #e0e0e0; font-family: 'Courier New', monospace; }}
  #container {{ max-width: 800px; margin: 40px auto; padding: 20px; }}
  h1 {{ color: #e94560; margin-bottom: 20px; }}
  canvas {{
    width: 100%; max-width: 800px; height: 600px;
    background: #16213e; border: 2px solid #0f3460;
    display: block; margin: 20px 0;
  }}
  #status {{ padding: 10px; background: #16213e; border: 1px solid #0f3460; margin: 10px 0; white-space: pre-wrap; font-size: 14px; max-height: 300px; overflow-y: auto; }}
  .pass {{ color: #4ecca3; }}
  .fail {{ color: #e94560; }}
  .info {{ color: #a0a0d0; }}
  button {{ background: #0f3460; color: #e0e0e0; border: 1px solid #e94560; padding: 8px 16px; cursor: pointer; font-family: inherit; margin: 4px; }}
  button:hover {{ background: #e94560; }}
</style>
</head>
<body>
<div id="container">
  <h1>Qwasm2-rs</h1>
  <div>
    <button onclick="runSelfTest()">Run Self-Test</button>
    <button onclick="checkWebGL()">Check WebGL2</button>
    <button onclick="showEngineInfo()">Engine Info</button>
  </div>
  <canvas id="canvas" width="800" height="600"></canvas>
  <div id="status"><span class="info">Loading WASM module...</span></div>
</div>

<script type="module">
{inlined_js}

const statusEl = document.getElementById('status');

function log(msg, cls = 'info') {{
  const span = document.createElement('span');
  span.className = cls;
  span.textContent = msg + '\n';
  statusEl.appendChild(span);
  statusEl.scrollTop = statusEl.scrollHeight;
}}

// Expose to global scope for button onclick handlers
window.runSelfTest = function() {{
  try {{
    const result = self_test();
    if (result === 'PASS') {{
      log('Self-test: PASS', 'pass');
    }} else {{
      log('Self-test: ' + result, 'fail');
    }}
  }} catch (e) {{
    log('Self-test error: ' + e, 'fail');
  }}
}};

window.checkWebGL = function() {{
  try {{
    log(check_webgl2());
  }} catch (e) {{
    log('WebGL check error: ' + e, 'fail');
  }}
}};

window.showEngineInfo = function() {{
  try {{
    log(engine_info());
  }} catch (e) {{
    log('Engine info error: ' + e, 'fail');
  }}
}};

// Initialize WASM module
try {{
  await init();
  log('WASM module loaded: ' + engine_version(), 'pass');

  // Auto-run self-test
  window.runSelfTest();
  window.checkWebGL();

  // Playwright test hooks (same scope so functions are accessible)
  document.getElementById('wasm-loaded').textContent = 'true';
  try {{
    document.getElementById('self-test-result').textContent = self_test();
  }} catch(e) {{
    document.getElementById('self-test-result').textContent = 'ERROR: ' + e;
  }}
}} catch (e) {{
  log('Failed to initialize WASM: ' + e, 'fail');
  document.getElementById('self-test-result').textContent = 'ERROR: ' + e;
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
