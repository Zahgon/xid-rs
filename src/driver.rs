//! The `database/sql` glue types.
//!
//! Go's `Value()` returns a `driver.Value` (an `interface{}` restricted to a
//! handful of types) and `Scan()` accepts a bare `interface{}`. Rust has no
//! dynamic `interface{}`, so both sides are modelled as enums:
//!
//! * [`DriverValue`] — what `Value()` may return: `nil`, a `string`, or a
//!   `[]byte` (the `b` sub-module returns the byte form).
//! * [`ScanValue`] — what `Scan()` may receive. Each variant remembers the Go
//!   type it stands for so `Scan` can reproduce
//!   `fmt.Errorf("xid: scanning unsupported type: %T", value)` verbatim.

/// Port of Go's `driver.Value` for the subset `xid` produces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriverValue {
    /// Go's `nil`.
    Null,
    /// Go's `string`.
    Str(String),
    /// Go's `[]byte`.
    Bytes(Vec<u8>),
}

impl DriverValue {
    /// True when the value is Go's `nil`.
    pub fn is_null(&self) -> bool {
        matches!(self, DriverValue::Null)
    }

    /// The string payload, if this is a `string` value.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            DriverValue::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// The byte payload, if this is a `[]byte` value.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            DriverValue::Bytes(b) => Some(b.as_slice()),
            _ => None,
        }
    }
}

impl PartialEq<str> for DriverValue {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == Some(other)
    }
}

impl PartialEq<&str> for DriverValue {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == Some(*other)
    }
}

impl PartialEq<[u8]> for DriverValue {
    fn eq(&self, other: &[u8]) -> bool {
        self.as_bytes() == Some(other)
    }
}

/// Port of the `interface{}` argument of `Scan`.
///
/// The variants cover Go's `driver.Value` set plus the plain `int` the Go test
/// suite feeds to `Scan` to trigger the "unsupported type" error.
#[derive(Debug, Clone, PartialEq)]
pub enum ScanValue<'a> {
    /// Go's `nil`.
    Nil,
    /// Go's `string`.
    Str(&'a str),
    /// Go's `[]byte`.
    Bytes(&'a [u8]),
    /// Go's `int`.
    Int(i64),
    /// Go's `int64`.
    Int64(i64),
    /// Go's `float64`.
    Float64(f64),
    /// Go's `bool`.
    Bool(bool),
}

impl ScanValue<'_> {
    /// The Go type name, i.e. what `%T` would print for this value.
    pub fn go_type_name(&self) -> &'static str {
        match self {
            ScanValue::Nil => "<nil>",
            ScanValue::Str(_) => "string",
            ScanValue::Bytes(_) => "[]uint8",
            ScanValue::Int(_) => "int",
            ScanValue::Int64(_) => "int64",
            ScanValue::Float64(_) => "float64",
            ScanValue::Bool(_) => "bool",
        }
    }
}

impl<'a> From<&'a str> for ScanValue<'a> {
    fn from(v: &'a str) -> Self {
        ScanValue::Str(v)
    }
}

impl<'a> From<&'a String> for ScanValue<'a> {
    fn from(v: &'a String) -> Self {
        ScanValue::Str(v.as_str())
    }
}

impl<'a> From<&'a [u8]> for ScanValue<'a> {
    fn from(v: &'a [u8]) -> Self {
        ScanValue::Bytes(v)
    }
}

impl<'a> From<&'a Vec<u8>> for ScanValue<'a> {
    fn from(v: &'a Vec<u8>) -> Self {
        ScanValue::Bytes(v.as_slice())
    }
}

impl<'a, const N: usize> From<&'a [u8; N]> for ScanValue<'a> {
    fn from(v: &'a [u8; N]) -> Self {
        ScanValue::Bytes(v.as_slice())
    }
}

impl From<i32> for ScanValue<'_> {
    /// An untyped Go integer literal (`id.Scan(0)`) has type `int`.
    fn from(v: i32) -> Self {
        ScanValue::Int(v as i64)
    }
}

impl From<i64> for ScanValue<'_> {
    fn from(v: i64) -> Self {
        ScanValue::Int64(v)
    }
}

impl From<f64> for ScanValue<'_> {
    fn from(v: f64) -> Self {
        ScanValue::Float64(v)
    }
}

impl From<bool> for ScanValue<'_> {
    fn from(v: bool) -> Self {
        ScanValue::Bool(v)
    }
}

impl From<()> for ScanValue<'_> {
    /// Go's `nil`.
    fn from(_: ()) -> Self {
        ScanValue::Nil
    }
}

impl<'a, T> From<Option<T>> for ScanValue<'a>
where
    T: Into<ScanValue<'a>>,
{
    /// A `None` scans as Go's `nil`.
    fn from(v: Option<T>) -> Self {
        match v {
            Some(v) => v.into(),
            None => ScanValue::Nil,
        }
    }
}
