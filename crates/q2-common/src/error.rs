//! Engine error type, defined in q2-shared so q2-render-api, which sits
//! beside q2-common rather than above it, can return it too.

pub use q2_shared::error::{Q2Error, Q2Result};

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
}
