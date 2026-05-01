use thiserror::Error;

/// Replaces Com_Error() + ERR_FATAL/ERR_DROP/ERR_QUIT.
/// The frame loop catches Drop errors and continues.
/// Fatal errors terminate the process.
#[derive(Error, Debug)]
pub enum Q2Error {
    /// ERR_DROP: Print to console, disconnect, continue main loop.
    #[error("drop: {0}")]
    Drop(String),

    /// ERR_FATAL: Exit the entire game.
    #[error("fatal: {0}")]
    Fatal(String),

    /// ERR_QUIT: Clean shutdown requested.
    #[error("quit")]
    Quit,

    /// Generic I/O error wrapper.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// Network error.
    #[error("net: {0}")]
    Net(String),
}

impl Q2Error {
    pub fn is_recoverable(&self) -> bool {
        matches!(self, Q2Error::Drop(_))
    }
}

pub type Q2Result<T> = Result<T, Q2Error>;

/// Replacement for Com_Error(ERR_DROP, ...) -- returns Err(Drop).
#[macro_export]
macro_rules! q2_drop {
    ($($arg:tt)*) => {
        return Err($crate::error::Q2Error::Drop(format!($($arg)*)))
    };
}

/// Replacement for Com_Error(ERR_FATAL, ...) -- returns Err(Fatal).
#[macro_export]
macro_rules! q2_fatal {
    ($($arg:tt)*) => {
        return Err($crate::error::Q2Error::Fatal(format!($($arg)*)))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_drop_is_recoverable() {
        let err = Q2Error::Drop("connection lost".into());
        assert!(err.is_recoverable());
    }

    #[test]
    fn error_fatal_is_not_recoverable() {
        let err = Q2Error::Fatal("out of memory".into());
        assert!(!err.is_recoverable());
    }

    #[test]
    fn error_quit_is_not_recoverable() {
        let err = Q2Error::Quit;
        assert!(!err.is_recoverable());
    }

    #[test]
    fn error_io_is_not_recoverable() {
        let err = Q2Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file not found",
        ));
        assert!(!err.is_recoverable());
    }

    #[test]
    fn error_display() {
        let err = Q2Error::Drop("test message".into());
        assert_eq!(format!("{err}"), "drop: test message");
    }

    #[test]
    fn io_error_converts() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
        let q2_err: Q2Error = io_err.into();
        assert!(matches!(q2_err, Q2Error::Io(_)));
    }

    #[test]
    fn q2result_type_works() {
        fn might_fail(fail: bool) -> Q2Result<i32> {
            if fail {
                Err(Q2Error::Drop("nope".into()))
            } else {
                Ok(42)
            }
        }
        assert_eq!(might_fail(false).unwrap(), 42);
        assert!(might_fail(true).is_err());
    }

    #[test]
    fn q2_drop_macro() {
        fn test_drop() -> Q2Result<()> {
            q2_drop!("connection {} lost", "test");
        }
        let err = test_drop().unwrap_err();
        assert!(matches!(err, Q2Error::Drop(ref msg) if msg == "connection test lost"));
    }

    #[test]
    fn q2_fatal_macro() {
        fn test_fatal() -> Q2Result<()> {
            q2_fatal!("out of memory: {} bytes", 1024);
        }
        let err = test_fatal().unwrap_err();
        assert!(matches!(err, Q2Error::Fatal(ref msg) if msg == "out of memory: 1024 bytes"));
    }

    #[test]
    fn frame_loop_error_recovery() {
        // Simulates the main game loop catching ERR_DROP and continuing
        let mut recovered = false;
        for _ in 0..3 {
            let result: Q2Result<()> = Err(Q2Error::Drop("frame error".into()));
            match result {
                Ok(()) => {}
                Err(ref e) if e.is_recoverable() => {
                    recovered = true;
                    continue; // Skip to next frame
                }
                Err(e) => panic!("fatal: {e}"),
            }
        }
        assert!(recovered);
    }
}
