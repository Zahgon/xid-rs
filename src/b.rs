//! Bytes storage — port of the `b` sub-package (Go package `xidb`).
//!
//! This submodule is there to allow storage of XIDs in a binary format in, for
//! example, a database. It allows some data size optimisation as the 12 bytes
//! will be smaller to store than a string.
//!
//! Go embeds `xid.ID` in a struct, which promotes every method of the inner
//! type and shadows only `Value`/`Scan`. Rust has no embedding, so the inner
//! id is a named field (spelled `id`, where Go spells it `ID`) and `Deref`
//! provides the same method promotion:
//!
//! ```
//! let inner = xid::from_string("9m4e2mr0ui3e8a215n4g").unwrap();
//! let id = xid::b::ID { id: inner };
//! assert_eq!(id.string(), "9m4e2mr0ui3e8a215n4g"); // promoted from xid::ID
//! assert_eq!(id.value().unwrap(), xid::DriverValue::Bytes(inner.bytes().to_vec()));
//! ```

use crate::driver::{DriverValue, ScanValue};
use crate::error::Error;
use std::fmt;
use std::ops::{Deref, DerefMut};

/// Go's `type ID struct { xid.ID }`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
#[repr(transparent)]
pub struct ID {
    /// The embedded `xid.ID`.
    pub id: crate::ID,
}

impl ID {
    /// Wraps a plain [`crate::ID`].
    pub fn new(id: crate::ID) -> Self {
        ID { id }
    }

    /// `Value` implements the driver.Valuer interface.
    ///
    /// Unlike [`crate::ID::value`], this yields the raw 12 bytes.
    pub fn value(&self) -> Result<DriverValue, Error> {
        if self.id.is_nil() {
            return Ok(DriverValue::Null);
        }
        Ok(DriverValue::Bytes(self.id.0.to_vec()))
    }

    /// `Scan` implements the sql.Scanner interface.
    pub fn scan<'a, V: Into<ScanValue<'a>>>(&mut self, value: V) -> Result<(), Error> {
        let value = value.into();
        match value {
            ScanValue::Bytes(val) => {
                let i = crate::from_bytes(val)?;
                *self = ID { id: i };
                Ok(())
            }
            ScanValue::Nil => {
                *self = ID {
                    id: crate::nil_id(),
                };
                Ok(())
            }
            other => Err(Error::ScanUnsupportedType(other.go_type_name().to_string())),
        }
    }
}

/// Emulates Go's struct embedding: every `xid::ID` method is callable on a
/// `b::ID`, except the two shadowed above.
impl Deref for ID {
    type Target = crate::ID;

    fn deref(&self) -> &crate::ID {
        &self.id
    }
}

impl DerefMut for ID {
    fn deref_mut(&mut self) -> &mut crate::ID {
        &mut self.id
    }
}

impl From<crate::ID> for ID {
    fn from(id: crate::ID) -> Self {
        ID { id }
    }
}

impl From<ID> for crate::ID {
    fn from(id: ID) -> Self {
        id.id
    }
}

/// Promoted from the embedded `xid.ID`, like Go's method set.
impl fmt::Display for ID {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.id, f)
    }
}

impl fmt::Debug for ID {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.id, f)
    }
}
