//! `tracing` output to the browser console.
//!
//! Where C raises `ERR_DROP`/`ERR_FATAL` the server and game log a
//! `tracing` error and carry on; without a subscriber the browser build
//! would drop those lines. Errors and warnings are shown; info and debug
//! stand in for C's developer prints (`Com_DPrintf`), which C only shows
//! with `developer` set.

use std::io;

use tracing::{Level, Metadata, Subscriber};
use tracing_subscriber::fmt::MakeWriter;

/// Collects one formatted event and hands it to `sink` with the event's
/// level when the formatter drops the writer.
struct LineWriter<'a, S: Fn(Level, &str)> {
    level: Level,
    line: Vec<u8>,
    sink: &'a S,
}

impl<S: Fn(Level, &str)> io::Write for LineWriter<'_, S> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.line.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<S: Fn(Level, &str)> Drop for LineWriter<'_, S> {
    fn drop(&mut self) {
        let line = String::from_utf8_lossy(&self.line);
        let line = line.trim_end();
        if !line.is_empty() {
            (self.sink)(self.level, line);
        }
    }
}

struct LineSink<S>(S);

impl<'a, S: Fn(Level, &str) + 'a> MakeWriter<'a> for LineSink<S> {
    type Writer = LineWriter<'a, S>;

    fn make_writer(&'a self) -> Self::Writer {
        self.make_writer_for_level(Level::INFO)
    }

    fn make_writer_for(&'a self, meta: &Metadata<'_>) -> Self::Writer {
        self.make_writer_for_level(*meta.level())
    }
}

impl<S> LineSink<S> {
    fn make_writer_for_level(&self, level: Level) -> LineWriter<'_, S>
    where
        S: Fn(Level, &str),
    {
        LineWriter {
            level,
            line: Vec::new(),
            sink: &self.0,
        }
    }
}

/// A subscriber that formats each warning or error as one line for
/// `sink`. No timestamps: `SystemTime::now` panics on
/// wasm32-unknown-unknown, and the console stamps lines itself.
pub(crate) fn console_subscriber<S>(sink: S) -> impl Subscriber + Send + Sync + 'static
where
    S: Fn(Level, &str) + Send + Sync + 'static,
{
    tracing_subscriber::fmt()
        .with_writer(LineSink(sink))
        .with_max_level(Level::WARN)
        .with_ansi(false)
        .without_time()
        .finish()
}

/// The browser console sink: errors to `console.error`, warnings to
/// `console.warn`.
#[cfg(target_arch = "wasm32")]
pub(crate) fn browser_console(level: Level, line: &str) {
    let line = wasm_bindgen::JsValue::from_str(line);
    match level {
        Level::ERROR => web_sys::console::error_1(&line),
        Level::WARN => web_sys::console::warn_1(&line),
        _ => web_sys::console::log_1(&line),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tracing::Level;

    type Lines = Arc<Mutex<Vec<(Level, String)>>>;

    fn recording() -> (Lines, impl Fn(Level, &str) + Send + Sync + 'static) {
        let lines = Lines::default();
        let sink_lines = lines.clone();
        let sink = move |level: Level, line: &str| {
            sink_lines
                .lock()
                .expect("sink lock")
                .push((level, line.to_string()));
        };
        (lines, sink)
    }

    #[test]
    fn errors_and_warnings_reach_the_sink_once_each_at_their_level() {
        let (lines, sink) = recording();
        tracing::subscriber::with_default(console_subscriber(sink), || {
            tracing::error!(name = "x", "index overflow (C ERR_DROP)");
            tracing::warn!("area_bits: bad area");
        });
        let lines = lines.lock().expect("lines lock");
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert_eq!(lines[0].0, Level::ERROR);
        assert!(lines[0].1.contains("index overflow (C ERR_DROP)"));
        assert!(lines[0].1.contains("name=\"x\""));
        assert!(!lines[0].1.ends_with('\n'));
        assert_eq!(lines[1].0, Level::WARN);
        assert!(lines[1].1.contains("area_bits: bad area"));
    }

    #[test]
    fn info_and_debug_are_left_out_like_c_developer_prints() {
        let (lines, sink) = recording();
        tracing::subscriber::with_default(console_subscriber(sink), || {
            tracing::info!("loaded");
            tracing::debug!("teleporter without a target");
        });
        assert!(lines.lock().expect("lines lock").is_empty());
    }
}
