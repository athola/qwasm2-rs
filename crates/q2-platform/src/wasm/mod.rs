pub mod game_loop;
pub mod gl_context;
pub mod input;
pub mod pak;

use q2_common::error::Q2Error;

/// Browser APIs failing at startup (no window, no WebGL2) leave the engine
/// unable to run, as C Sys_Error does for video init.
pub(crate) fn fatal(msg: impl Into<String>) -> Q2Error {
    Q2Error::Fatal(msg.into())
}
