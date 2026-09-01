// Copyright (c) 2015 Olivier Poitrey <rs@dailymotion.com>
// Licensed under the MIT License. See LICENSE for details.

//! Package xid is a globally unique id generator suited for web scale
//!
//! Xid is using Mongo Object ID algorithm to generate globally unique ids:
//! <https://docs.mongodb.org/manual/reference/object-id/>
//!
//!   - 4-byte value representing the seconds since the Unix epoch,
//!   - 3-byte machine identifier,
//!   - 2-byte process id, and
//!   - 3-byte counter, starting with a random value.
//!
//! The binary representation of the id is compatible with Mongo 12 bytes Object IDs.
//! The string representation is using base32 hex (w/o padding) for better space efficiency
//! when stored in that form (20 bytes). The hex variant of base32 is used to retain the
//! sortable property of the id.
//!
//! Xid doesn't use base64 because case sensitivity and the 2 non alphanum chars may be an
//! issue when transported as a string between various systems. Base36 wasn't retained either
//! because 1/ it's not standard 2/ the resulting size is not predictable (not bit aligned)
//! and 3/ it would not remain sortable. To validate a base32 `xid`, expect a 20 chars long,
//! all lowercase sequence of `a` to `v` letters and `0` to `9` numbers (`[0-9a-v]{20}`).
//!
//! UUID is 16 bytes (128 bits), snowflake is 8 bytes (64 bits), xid stands in between
//! with 12 bytes with a more compact string representation ready for the web and no
//! required configuration or central generation server.
//!
//! Features:
//!
//!   - Size: 12 bytes (96 bits), smaller than UUID, larger than snowflake
//!   - Base32 hex encoded by default (16 bytes storage when transported as printable string)
//!   - Non configured, you don't need set a unique machine and/or data center id
//!   - K-ordered
//!   - Embedded time with 1 second precision
//!   - Unicity guaranteed for 16,777,216 (24 bits) unique ids per second and per host/process
//!
//! # A 1:1 port of `github.com/rs/xid`
//!
//! Every exported Go identifier has a counterpart here, the bit-level encoding
//! is identical, and the error values, panic messages and edge-case behaviour
//! are reproduced exactly.
//!
//! | Go                          | Rust                                   |
//! |-----------------------------|----------------------------------------|
//! | `xid.ID`                    | [`ID`]                                 |
//! | `xid.New()`                 | [`new()`] / [`ID::new`]                |
//! | `xid.NewWithTime(t)`        | [`new_with_time`] / [`ID::new_with_time`] |
//! | `xid.FromString(s)`         | [`from_string`] / [`ID::from_string`] / [`str::parse`] |
//! | `xid.FromBytes(b)`          | [`from_bytes`] / [`ID::from_bytes`]    |
//! | `xid.NilID()`               | [`nil_id`] / [`ID::default`]           |
//! | `xid.Sort(ids)`             | [`sort`]                               |
//! | `xid.ErrInvalidID`          | [`ERR_INVALID_ID`] / [`Error::InvalidId`] |
//! | `id.String()`               | [`ID::string`] / `Display`             |
//! | `id.Encode(dst)`            | [`ID::encode`]                         |
//! | `id.MarshalText()`          | [`ID::marshal_text`]                   |
//! | `id.UnmarshalText(b)`       | [`ID::unmarshal_text`]                 |
//! | `id.MarshalJSON()`          | [`ID::marshal_json`]                   |
//! | `id.UnmarshalJSON(b)`       | [`ID::unmarshal_json`]                 |
//! | `id.Time()`                 | [`ID::time`]                           |
//! | `id.Machine()`              | [`ID::machine`]                        |
//! | `id.Pid()`                  | [`ID::pid`]                            |
//! | `id.Counter()`              | [`ID::counter`]                        |
//! | `id.Value()`                | [`ID::value`]                          |
//! | `id.Scan(v)`                | [`ID::scan`]                           |
//! | `id.IsNil()` / `id.IsZero()`| [`ID::is_nil`] / [`ID::is_zero`]       |
//! | `id.Bytes()`                | [`ID::bytes`]                          |
//! | `id.Compare(other)`         | [`ID::compare`]                        |
//! | `xidb.ID` (package `b`)     | [`b::ID`]                              |
//!
//! ```
//! let guid = xid::new();
//! println!("{guid}");
//! // Output: 9m4e2mr0ui3e8a215n4g
//! ```

pub mod b;
pub mod driver;
pub mod error;
pub mod gocrc32;
pub mod goos;
pub mod gorand;
pub mod gosha256;
pub mod hostid;

pub use driver::{DriverValue, ScanValue};
pub use error::{Error, ERR_INVALID_ID};

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;
use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// Code inspired from mgo/bson ObjectId

/// String encoded len.
pub const ENCODED_LEN: usize = 20;
/// Binary raw len.
pub const RAW_LEN: usize = 12;

/// `encoding` stores a custom version of the base32 encoding with lower case
/// letters.
pub const ENCODING: &[u8; 32] = b"0123456789abcdefghijklmnopqrstuv";

/// `dec` is the decoding map for base32 encoding.
///
/// Go fills this in `init()`; Rust builds it at compile time, with the same
/// contents: `0xFF` for every byte that is not part of `ENCODING`.
static DEC: [u8; 256] = make_dec();

const fn make_dec() -> [u8; 256] {
    let mut dec = [0xFFu8; 256];
    let mut i = 0;
    while i < ENCODING.len() {
        dec[ENCODING[i] as usize] = i as u8;
        i += 1;
    }
    dec
}

/// `objectIDCounter` is atomically incremented when generating a new ObjectId.
/// It's used as the counter part of an id. This id is initialized with a
/// random value.
///
/// Go initialises package-level vars before `main`; Rust has no life-before-
/// main, so the initialisation is deferred to first use. The observable
/// behaviour is the same: one random seed per process, incremented atomically.
static OBJECT_ID_COUNTER: OnceLock<AtomicU32> = OnceLock::new();

/// `machineID` is generated once and used in subsequent calls to the New*
/// functions.
static MACHINE_ID: OnceLock<[u8; 3]> = OnceLock::new();

/// `pid` stores the current process id (already xor-ed with the cpuset CRC
/// when running in a container, see [`init_pid`]).
static PID: OnceLock<i64> = OnceLock::new();

/// The zero value, Go's `nilID`.
const NIL_ID: ID = ID([0u8; RAW_LEN]);

fn object_id_counter() -> &'static AtomicU32 {
    OBJECT_ID_COUNTER.get_or_init(|| AtomicU32::new(rand_int()))
}

fn machine_id() -> &'static [u8; 3] {
    MACHINE_ID.get_or_init(read_machine_id)
}

fn pid() -> i64 {
    *PID.get_or_init(init_pid)
}

/// Port of the second half of Go's `init()`.
///
/// If `/proc/self/cpuset` exists and is not `/`, we can assume that we are in a
/// form of container and use the content of cpuset xor-ed with the PID in
/// order get a reasonable machine global unique PID.
fn init_pid() -> i64 {
    let mut pid = goos::getpid();
    if let Ok(b) = goos::read_file("/proc/self/cpuset") {
        if b.len() > 1 {
            pid ^= gocrc32::checksum_ieee(&b) as i64;
        }
    }
    pid
}

/// `readMachineID` generates a machine ID, derived from a platform-specific
/// machine ID value, or else the machine's hostname, or else a
/// randomly-generated number. It panics if all of these methods fail.
fn read_machine_id() -> [u8; 3] {
    // Allow env overrides for the machine id
    if let Some(id) = read_machine_id_from_env() {
        return id;
    }

    let mut id = [0u8; 3];
    let mut err: Option<std::io::Error> = None;
    let mut hid = match hostid::read_platform_machine_id() {
        Ok(hid) => hid,
        Err(e) => {
            err = Some(e);
            String::new()
        }
    };
    if err.is_some() || hid.is_empty() {
        err = None;
        hid = match goos::hostname() {
            Ok(h) => h,
            Err(e) => {
                err = Some(e);
                String::new()
            }
        };
    }
    if err.is_none() && !hid.is_empty() {
        let mut hw = gosha256::Digest::new();
        hw.write(hid.as_bytes());
        // Go: copy(id, hw.Sum(nil)) — copies as many bytes as `id` holds.
        id.copy_from_slice(&hw.sum()[..3]);
    } else {
        // Fallback to rand number if machine id can't be gathered
        if let Err(rand_err) = gorand::read(&mut id) {
            panic!(
                "xid: cannot get hostname nor generate a random number: {}; {}",
                go_error(&err),
                rand_err
            );
        }
    }
    id
}

/// Formats an optional error the way Go's `%v` verb does.
fn go_error(err: &Option<std::io::Error>) -> String {
    match err {
        Some(e) => e.to_string(),
        None => "<nil>".to_string(),
    }
}

/// Port of Go's unexported `readMachineIDFromEnv`.
///
/// Returns `None` where Go returns a `nil` slice. Exposed (Go keeps it
/// package-private) so the ported test-suite can exercise it exactly like the
/// in-package Go test does.
///
/// # Panics
///
/// With `"XID_MACHINE_ID value is set to not a number"` when `XID_MACHINE_ID`
/// does not parse as an integer, and with `"XID_MACHINE_ID out of range for 3
/// bytes"` when it does not fit in three bytes — the same two messages, and
/// the same conditions, as Go.
pub fn read_machine_id_from_env() -> Option<[u8; 3]> {
    let env_machine_id = goos::getenv("XID_MACHINE_ID");
    if env_machine_id.is_empty() {
        return None;
    }

    // Go's strconv.Atoi: optional sign, decimal digits, nothing else; a value
    // that overflows `int` is an error too.
    let num: i64 = match env_machine_id.parse::<i64>() {
        Ok(n) => n,
        Err(_) => panic!("XID_MACHINE_ID value is set to not a number"),
    };

    if !(0..=0xFFFFFF).contains(&num) {
        panic!("XID_MACHINE_ID out of range for 3 bytes");
    }

    // Encode the number into big endian.
    Some([(num >> 16) as u8, (num >> 8) as u8, num as u8])
}

/// `randInt` generates a random uint32.
///
/// # Panics
///
/// If the platform's secure random source fails, mirroring Go's
/// `panic(fmt.Errorf("xid: cannot generate random number: %v;", err))`.
fn rand_int() -> u32 {
    let mut b = [0u8; 3];
    if let Err(err) = gorand::read(&mut b) {
        panic!("xid: cannot generate random number: {err};");
    }
    (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32
}

/// ID represents a unique request id.
///
/// The Go declaration is `type ID [rawLen]byte`; this is the same 12-byte
/// array, so it is `Copy`, compares lexicographically and hashes like the
/// array it wraps.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct ID(pub [u8; RAW_LEN]);

/// Go's zero value for `ID` is `nilID`.
impl Default for ID {
    fn default() -> Self {
        NIL_ID
    }
}

/// `New` generates a globally unique ID.
pub fn new() -> ID {
    new_with_time(SystemTime::now())
}

/// `NewWithTime` generates a globally unique ID with the passed in time.
pub fn new_with_time(t: SystemTime) -> ID {
    let mut id = [0u8; RAW_LEN];
    // Timestamp, 4 bytes, big endian
    id[0..4].copy_from_slice(&(go_unix_seconds(t) as u32).to_be_bytes());
    // Machine ID, 3 bytes
    let machine = machine_id();
    id[4] = machine[0];
    id[5] = machine[1];
    id[6] = machine[2];
    // Pid, 2 bytes, specs don't specify endianness, but we use big endian.
    let pid = pid();
    id[7] = (pid >> 8) as u8;
    id[8] = pid as u8;
    // Increment, 3 bytes, big endian
    // Go's atomic.AddUint32 returns the *new* value; fetch_add returns the old.
    let i = object_id_counter()
        .fetch_add(1, AtomicOrdering::SeqCst)
        .wrapping_add(1);
    id[9] = (i >> 16) as u8;
    id[10] = (i >> 8) as u8;
    id[11] = i as u8;
    ID(id)
}

/// `FromString` reads an ID from its string representation.
pub fn from_string(id: &str) -> Result<ID, Error> {
    // Go builds a zero ID, calls UnmarshalText and returns *both* the (possibly
    // partially written, then reset) id and the error.
    let mut i = ID::default();
    let err = i.unmarshal_text(id.as_bytes());
    match err {
        Ok(()) => Ok(i),
        Err(e) => Err(e),
    }
}

/// `FromBytes` convert the byte array representation of `ID` back to `ID`.
pub fn from_bytes(b: &[u8]) -> Result<ID, Error> {
    let mut id = ID::default();
    if b.len() != RAW_LEN {
        return Err(Error::InvalidId);
    }
    id.0.copy_from_slice(b);
    Ok(id)
}

/// `NilID` returns a zero value for `xid.ID`.
pub fn nil_id() -> ID {
    NIL_ID
}

/// `Sort` sorts an array of IDs inplace.
///
/// Go wraps `[]ID` in `sorter` and calls `sort.Sort`; the comparison is
/// `Compare(...) < 0`, i.e. plain lexicographic byte order — which is exactly
/// the derived `Ord` of the wrapped array. `sort.Sort` is not stable either,
/// and equal ids are byte-identical, so the outcome is the same.
pub fn sort(ids: &mut [ID]) {
    ids.sort_unstable();
}

impl ID {
    /// `New` generates a globally unique ID.
    pub fn new() -> ID {
        new()
    }

    /// `NewWithTime` generates a globally unique ID with the passed in time.
    pub fn new_with_time(t: SystemTime) -> ID {
        new_with_time(t)
    }

    /// `FromString` reads an ID from its string representation.
    pub fn from_string(id: &str) -> Result<ID, Error> {
        from_string(id)
    }

    /// `FromBytes` convert the byte array representation of `ID` back to `ID`.
    pub fn from_bytes(b: &[u8]) -> Result<ID, Error> {
        from_bytes(b)
    }

    /// `NilID` returns a zero value for `xid.ID`.
    pub fn nil() -> ID {
        NIL_ID
    }

    /// `String` returns a base32 hex lowercased with no padding representation
    /// of the id (char set is 0-9, a-v).
    pub fn string(&self) -> String {
        let mut text = [0u8; ENCODED_LEN];
        encode(&mut text, &self.0);
        // Every byte comes from `ENCODING`, so this is always valid ASCII.
        String::from_utf8(text.to_vec()).expect("base32 output is ASCII")
    }

    /// `Encode` encodes the id using base32 encoding, writing 20 bytes to dst
    /// and return it.
    ///
    /// # Panics
    ///
    /// If `dst` is shorter than [`ENCODED_LEN`], matching Go's index panic.
    pub fn encode<'a>(&self, dst: &'a mut [u8]) -> &'a mut [u8] {
        encode(dst, &self.0);
        dst
    }

    /// `MarshalText` implements encoding/text TextMarshaler interface.
    pub fn marshal_text(&self) -> Result<Vec<u8>, Error> {
        let mut text = vec![0u8; ENCODED_LEN];
        encode(&mut text, &self.0);
        Ok(text)
    }

    /// `MarshalJSON` implements encoding/json Marshaler interface.
    pub fn marshal_json(&self) -> Result<Vec<u8>, Error> {
        if self.is_nil() {
            return Ok(b"null".to_vec());
        }
        let mut text = vec![0u8; ENCODED_LEN + 2];
        encode(&mut text[1..ENCODED_LEN + 1], &self.0);
        text[0] = b'"';
        text[ENCODED_LEN + 1] = b'"';
        Ok(text)
    }

    /// `UnmarshalText` implements encoding/text TextUnmarshaler interface.
    pub fn unmarshal_text(&mut self, text: &[u8]) -> Result<(), Error> {
        if text.len() != ENCODED_LEN {
            return Err(Error::InvalidId);
        }
        for &c in text {
            if DEC[c as usize] == 0xFF {
                return Err(Error::InvalidId);
            }
        }
        if !decode(self, text) {
            *self = NIL_ID;
            return Err(Error::InvalidId);
        }
        Ok(())
    }

    /// `UnmarshalJSON` implements encoding/json Unmarshaler interface.
    pub fn unmarshal_json(&mut self, b: &[u8]) -> Result<(), Error> {
        if b == b"null" {
            *self = NIL_ID;
            return Ok(());
        }
        // Check the slice length to prevent panic on passing it to UnmarshalText()
        if b.len() < 2 {
            return Err(Error::InvalidId);
        }
        self.unmarshal_text(&b[1..b.len() - 1])
    }

    /// `Time` returns the timestamp part of the id.
    ///
    /// It's a runtime error to call this method with an invalid id.
    pub fn time(&self) -> SystemTime {
        // First 4 bytes of ObjectId is 32-bit big-endian seconds from epoch.
        let secs = u32::from_be_bytes([self.0[0], self.0[1], self.0[2], self.0[3]]) as u64;
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    /// The timestamp part of the id as seconds since the Unix epoch.
    ///
    /// Convenience accessor for the value Go reads with
    /// `binary.BigEndian.Uint32(id[0:4])`.
    pub fn unix_seconds(&self) -> i64 {
        u32::from_be_bytes([self.0[0], self.0[1], self.0[2], self.0[3]]) as i64
    }

    /// `Machine` returns the 3-byte machine id part of the id.
    ///
    /// It's a runtime error to call this method with an invalid id.
    pub fn machine(&self) -> &[u8] {
        &self.0[4..7]
    }

    /// `Pid` returns the process id part of the id.
    ///
    /// It's a runtime error to call this method with an invalid id.
    pub fn pid(&self) -> u16 {
        u16::from_be_bytes([self.0[7], self.0[8]])
    }

    /// `Counter` returns the incrementing value part of the id.
    ///
    /// It's a runtime error to call this method with an invalid id.
    pub fn counter(&self) -> i32 {
        let b = &self.0[9..12];
        // Counter is stored as big-endian 3-byte value
        ((b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32) as i32
    }

    /// `Value` implements the driver.Valuer interface.
    pub fn value(&self) -> Result<DriverValue, Error> {
        if self.is_nil() {
            return Ok(DriverValue::Null);
        }
        let b = self.marshal_text()?;
        Ok(DriverValue::Str(
            String::from_utf8(b).expect("base32 output is ASCII"),
        ))
    }

    /// `Scan` implements the sql.Scanner interface.
    pub fn scan<'a, V: Into<ScanValue<'a>>>(&mut self, value: V) -> Result<(), Error> {
        let value = value.into();
        match value {
            ScanValue::Str(val) => self.unmarshal_text(val.as_bytes()),
            ScanValue::Bytes(val) => {
                // BYTEA / binary columns yield raw 12-byte IDs; text/varchar yield
                // the base32 encoding. Accept both so Scan matches FromBytes/FromString.
                if val.len() == RAW_LEN {
                    self.0.copy_from_slice(val);
                    return Ok(());
                }
                self.unmarshal_text(val)
            }
            ScanValue::Nil => {
                *self = NIL_ID;
                Ok(())
            }
            other => Err(Error::ScanUnsupportedType(other.go_type_name().to_string())),
        }
    }

    /// `IsNil` Returns true if this is a "nil" ID.
    pub fn is_nil(&self) -> bool {
        *self == NIL_ID
    }

    /// Alias of [`ID::is_nil`].
    pub fn is_zero(&self) -> bool {
        self.is_nil()
    }

    /// `Bytes` returns the byte array representation of `ID`.
    pub fn bytes(&self) -> &[u8] {
        &self.0
    }

    /// `Compare` returns an integer comparing two IDs. It behaves just like
    /// `bytes.Compare`. The result will be 0 if two IDs are identical, -1 if
    /// current id is less than the other one, and 1 if current id is greater
    /// than the other.
    pub fn compare(&self, other: &ID) -> i32 {
        match self.0.cmp(&other.0) {
            Ordering::Less => -1,
            Ordering::Equal => 0,
            Ordering::Greater => 1,
        }
    }
}

/// `encode` by unrolling the stdlib base32 algorithm + removing all safe checks.
fn encode(dst: &mut [u8], id: &[u8]) {
    // Go's `_ = dst[19]` / `_ = id[11]` bounds hints; here they make the panic
    // happen up front, exactly as in Go, instead of midway through the writes.
    let _ = dst[19];
    let _ = id[11];

    dst[19] = ENCODING[((id[11] << 4) & 0x1F) as usize];
    dst[18] = ENCODING[((id[11] >> 1) & 0x1F) as usize];
    dst[17] = ENCODING[((id[11] >> 6) | ((id[10] << 2) & 0x1F)) as usize];
    dst[16] = ENCODING[(id[10] >> 3) as usize];
    dst[15] = ENCODING[(id[9] & 0x1F) as usize];
    dst[14] = ENCODING[((id[9] >> 5) | ((id[8] << 3) & 0x1F)) as usize];
    dst[13] = ENCODING[((id[8] >> 2) & 0x1F) as usize];
    dst[12] = ENCODING[((id[8] >> 7) | ((id[7] << 1) & 0x1F)) as usize];
    dst[11] = ENCODING[((id[7] >> 4) | ((id[6] << 4) & 0x1F)) as usize];
    dst[10] = ENCODING[((id[6] >> 1) & 0x1F) as usize];
    dst[9] = ENCODING[((id[6] >> 6) | ((id[5] << 2) & 0x1F)) as usize];
    dst[8] = ENCODING[(id[5] >> 3) as usize];
    dst[7] = ENCODING[(id[4] & 0x1F) as usize];
    dst[6] = ENCODING[((id[4] >> 5) | ((id[3] << 3) & 0x1F)) as usize];
    dst[5] = ENCODING[((id[3] >> 2) & 0x1F) as usize];
    dst[4] = ENCODING[((id[3] >> 7) | ((id[2] << 1) & 0x1F)) as usize];
    dst[3] = ENCODING[((id[2] >> 4) | ((id[1] << 4) & 0x1F)) as usize];
    dst[2] = ENCODING[((id[1] >> 1) & 0x1F) as usize];
    dst[1] = ENCODING[((id[1] >> 6) | ((id[0] << 2) & 0x1F)) as usize];
    dst[0] = ENCODING[(id[0] >> 3) as usize];
}

/// `decode` by unrolling the stdlib base32 algorithm + customized safe check.
fn decode(id: &mut ID, src: &[u8]) -> bool {
    let _ = src[19];
    let _ = id.0[11];

    let d = |i: usize| DEC[src[i] as usize];

    id.0[11] = (d(17) << 6) | (d(18) << 1) | (d(19) >> 4);
    // check the last byte
    if ENCODING[((id.0[11] << 4) & 0x1F) as usize] != src[19] {
        return false;
    }
    id.0[10] = (d(16) << 3) | (d(17) >> 2);
    id.0[9] = (d(14) << 5) | d(15);
    id.0[8] = (d(12) << 7) | (d(13) << 2) | (d(14) >> 3);
    id.0[7] = (d(11) << 4) | (d(12) >> 1);
    id.0[6] = (d(9) << 6) | (d(10) << 1) | (d(11) >> 4);
    id.0[5] = (d(8) << 3) | (d(9) >> 2);
    id.0[4] = (d(6) << 5) | d(7);
    id.0[3] = (d(4) << 7) | (d(5) << 2) | (d(6) >> 3);
    id.0[2] = (d(3) << 4) | (d(4) >> 1);
    id.0[1] = (d(1) << 6) | (d(2) << 1) | (d(3) >> 4);
    id.0[0] = (d(0) << 3) | (d(1) >> 2);
    true
}

/// Go's `time.Time.Unix()`: whole seconds since the epoch, rounded towards
/// negative infinity.
fn go_unix_seconds(t: SystemTime) -> i64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => {
            let d = e.duration();
            let mut secs = -(d.as_secs() as i64);
            if d.subsec_nanos() > 0 {
                secs -= 1;
            }
            secs
        }
    }
}

/// Port of Go's unexported `sorter` type (`type sorter []ID`).
///
/// Exposed so the ported test-suite can exercise `Len`/`Less`/`Swap` the way
/// the in-package Go test does.
pub struct Sorter<'a>(pub &'a mut [ID]);

impl Sorter<'_> {
    /// `sort.Interface.Len`.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// True when the wrapped slice is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// `sort.Interface.Less`.
    pub fn less(&self, i: usize, j: usize) -> bool {
        self.0[i].compare(&self.0[j]) < 0
    }

    /// `sort.Interface.Swap`.
    pub fn swap(&mut self, i: usize, j: usize) {
        self.0.swap(i, j);
    }
}

// ---------------------------------------------------------------------------
// Trait glue — the idiomatic Rust face of the Go methods above.
// ---------------------------------------------------------------------------

/// Go's `%v`/`%s` on an `ID` calls `String()`; keep the same rendering here so
/// error messages read identically.
impl fmt::Display for ID {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut text = [0u8; ENCODED_LEN];
        encode(&mut text, &self.0);
        f.write_str(std::str::from_utf8(&text).expect("base32 output is ASCII"))
    }
}

impl fmt::Debug for ID {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl FromStr for ID {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        from_string(s)
    }
}

impl TryFrom<&[u8]> for ID {
    type Error = Error;

    fn try_from(b: &[u8]) -> Result<Self, Error> {
        from_bytes(b)
    }
}

impl From<[u8; RAW_LEN]> for ID {
    fn from(b: [u8; RAW_LEN]) -> Self {
        ID(b)
    }
}

impl AsRef<[u8]> for ID {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl std::ops::Index<usize> for ID {
    type Output = u8;

    fn index(&self, i: usize) -> &u8 {
        &self.0[i]
    }
}

impl std::ops::IndexMut<usize> for ID {
    fn index_mut(&mut self, i: usize) -> &mut u8 {
        &mut self.0[i]
    }
}

// ---------------------------------------------------------------------------
// Unit tests for the internals Go exercises from inside its own package.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::hint::black_box;
    use std::sync::{Mutex, MutexGuard};

    /// `go test` runs a package's tests sequentially, so a Go test may poke at
    /// the shared counter. Rust runs them in parallel threads; this lock gives
    /// the generator-touching tests the same exclusivity.
    static GEN_LOCK: Mutex<()> = Mutex::new(());

    fn gen_lock() -> MutexGuard<'static, ()> {
        GEN_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The table Go fills in `init()`:
    ///
    /// ```go
    /// for i := 0; i < len(dec); i++ { dec[i] = 0xFF }
    /// for i := 0; i < len(encoding); i++ { dec[encoding[i]] = byte(i) }
    /// ```
    #[test]
    fn dec_table_matches_gos_init() {
        // Evaluated at runtime (not folded), so the const fn itself is tested.
        let dec = black_box(make_dec());
        assert_eq!(dec, DEC);

        for (i, &d) in dec.iter().enumerate() {
            let want = ENCODING
                .iter()
                .position(|&c| c as usize == i)
                .map(|p| p as u8)
                .unwrap_or(0xFF);
            assert_eq!(d, want, "dec[{i}]");
        }
        // Spot checks against the alphabet.
        assert_eq!(dec[b'0' as usize], 0);
        assert_eq!(dec[b'9' as usize], 9);
        assert_eq!(dec[b'a' as usize], 10);
        assert_eq!(dec[b'v' as usize], 31);
        assert_eq!(dec[b'w' as usize], 0xFF);
        assert_eq!(dec[b'A' as usize], 0xFF);
        assert_eq!(dec[0], 0xFF);
        assert_eq!(dec[255], 0xFF);
        assert_eq!(dec.iter().filter(|&&d| d != 0xFF).count(), 32);
    }

    /// `encode`/`decode` are Go's unrolled base32; they must be inverses for
    /// every canonical input.
    #[test]
    fn encode_decode_are_inverses() {
        let mut dst = [0u8; ENCODED_LEN];
        for i in 0..RAW_LEN {
            for v in 0..=255u8 {
                let mut raw = [0u8; RAW_LEN];
                raw[i] = v;
                encode(&mut dst, &raw);
                assert!(dst.iter().all(|c| ENCODING.contains(c)));
                let mut back = ID::default();
                assert!(decode(&mut back, &dst));
                assert_eq!(back.0, raw, "byte {i} = {v}");
            }
        }
    }

    /// Go's `%v` of a `nil` error prints `<nil>`; the panic message in
    /// `readMachineID` interpolates it.
    #[test]
    fn go_error_formats_like_the_v_verb() {
        assert_eq!(go_error(&None), "<nil>");
        let e = std::io::Error::new(std::io::ErrorKind::NotFound, "boom");
        assert_eq!(go_error(&Some(e)), "boom");
    }

    /// `uint32(t.Unix())` — floor division towards negative infinity, then a
    /// wrapping conversion.
    #[test]
    fn go_unix_seconds_floors_towards_negative_infinity() {
        assert_eq!(go_unix_seconds(UNIX_EPOCH), 0);
        assert_eq!(go_unix_seconds(UNIX_EPOCH + Duration::from_millis(1999)), 1);
        assert_eq!(go_unix_seconds(UNIX_EPOCH - Duration::from_secs(1)), -1);
        assert_eq!(go_unix_seconds(UNIX_EPOCH - Duration::from_millis(1)), -1);
        assert_eq!(
            go_unix_seconds(UNIX_EPOCH - Duration::from_millis(1500)),
            -2
        );
        assert_eq!(
            go_unix_seconds(UNIX_EPOCH + Duration::from_secs(1300816219)),
            1300816219
        );
    }

    /// `randInt` builds a 24-bit value from three random bytes.
    #[test]
    fn rand_int_is_24_bits() {
        for _ in 0..1000 {
            let v = rand_int();
            assert!(v <= 0xFF_FFFF, "randInt returned {v:#x}");
        }
        // Two draws are essentially never equal.
        assert!((0..8).any(|_| rand_int() != rand_int()));
    }

    /// The lazily initialised process id is the one `init()` computes.
    #[test]
    fn pid_is_initialised_like_gos_init() {
        let _guard = gen_lock();
        let want = init_pid();
        assert_eq!(pid(), want);
        assert_eq!(pid(), want, "stable across calls");
        assert_eq!(new().pid(), want as u16);
    }

    /// `machineID` is read once and reused by every generated id.
    #[test]
    fn machine_id_is_read_once() {
        let _guard = gen_lock();
        let first = *machine_id();
        assert_eq!(*machine_id(), first);
        assert_eq!(new().machine(), first);
    }

    /// The counter is seeded randomly and wraps like Go's `uint32`.
    #[test]
    fn counter_is_a_wrapping_u32() {
        let _guard = gen_lock();
        let c = object_id_counter();
        let a = c.fetch_add(1, AtomicOrdering::SeqCst).wrapping_add(1);
        let b = c.fetch_add(1, AtomicOrdering::SeqCst).wrapping_add(1);
        assert_eq!(b.wrapping_sub(a), 1);
        // Only the low 24 bits reach the id.
        c.store(0xFF_FFFF, AtomicOrdering::SeqCst);
        let id = new();
        assert_eq!(id.counter(), 0, "0xFFFFFF + 1 truncates to 0 in 3 bytes");
    }
}
