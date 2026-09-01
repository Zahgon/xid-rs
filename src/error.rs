//! Port of `error.go`.
//!
//! Go declares its sentinel as a constant string type so it can live in a
//! `const` block:
//!
//! ```go
//! const ErrInvalidID strErr = "xid: invalid ID"
//!
//! type strErr string
//! func (err strErr) Error() string { return string(err) }
//! ```
//!
//! The Rust equivalent is an enum whose `Display` output reproduces every
//! message the Go package can produce, and whose equality behaves like Go's
//! `err == ErrInvalidID` comparison.

use std::fmt;

/// Every error value the `xid` package can return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// `ErrInvalidID` — returned when trying to unmarshal an invalid ID.
    InvalidId,
    /// `fmt.Errorf("xid: scanning unsupported type: %T", value)` from
    /// [`crate::ID::scan`]. The payload is the Go type name (`%T`) of the
    /// scanned value, so the message is identical to Go's.
    ScanUnsupportedType(String),
}

/// `ErrInvalidID` is returned when trying to unmarshal an invalid ID.
///
/// Named to mirror the Go constant; `Error::InvalidId` is the same value.
pub const ERR_INVALID_ID: Error = Error::InvalidId;

impl Error {
    /// The Go `Error() string` method.
    pub fn error(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidId => f.write_str("xid: invalid ID"),
            Error::ScanUnsupportedType(t) => {
                write!(f, "xid: scanning unsupported type: {t}")
            }
        }
    }
}

impl std::error::Error for Error {}
