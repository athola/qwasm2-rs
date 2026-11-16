//! C PF_dprintf and PF_cprintf with no entity call Com_Printf, which always
//! reaches the console (not Com_DPrintf), so the game's prints log at INFO
//! under the `game` target.
//!
//! This is its own test binary on purpose: tracing caches whether each log
//! site is enabled process-wide, and a site first hit by a parallel test on a
//! thread with no subscriber could be cached as disabled while this test's
//! thread-scoped subscriber registers, dropping the event (about 1 run in 8
//! when this test lived among the other q2-server unit tests).

use std::io;
use std::sync::{Arc, Mutex};

use q2_game::traits::GameImport;
use q2_server::game_iface::GAME_PRINT_TARGET;
use q2_server::ServerGameImport;
use q2_shared::constants::PRINT_HIGH;

struct Shared(Arc<Mutex<Vec<u8>>>);

impl io::Write for Shared {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().expect("log lock").extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The `target level: message` of each event `run` logs at INFO or above.
fn logged(run: impl FnOnce()) -> Vec<String> {
    let out = Arc::new(Mutex::new(Vec::new()));
    let writer = out.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || Shared(writer.clone()))
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .without_time()
        .finish();
    tracing::subscriber::with_default(subscriber, run);
    let text = String::from_utf8(out.lock().expect("log lock").clone()).expect("utf-8");
    text.lines().map(|line| line.trim().to_string()).collect()
}

#[test]
fn dprintf_and_a_console_cprintf_log_at_info_under_the_game_target() {
    let gi = ServerGameImport::new();
    let lines = logged(|| {
        gi.dprintf("teleporter without a target\n");
        gi.cprintf(None, PRINT_HIGH, "console line\n");
    });
    assert_eq!(
        lines,
        [
            format!("INFO {GAME_PRINT_TARGET}: teleporter without a target"),
            format!("INFO {GAME_PRINT_TARGET}: console line"),
        ]
    );
}
