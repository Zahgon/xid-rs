//! Shared helpers for the ported test suites.
//!
//! Two Go standard-library facilities are used by `id_test.go` and have no
//! Rust equivalent, so they are reproduced here:
//!
//! * `math/rand` — only ever used for "give me an arbitrary value", never for
//!   an asserted output, so a small deterministic PRNG is enough (and makes
//!   any failure reproducible).
//! * `encoding/json` — the JSON tests exercise `MarshalJSON`/`UnmarshalJSON`
//!   through a struct. [`JsonType`] plus [`json_marshal`]/[`json_unmarshal`]
//!   reproduce exactly what `encoding/json` does for that struct: the raw
//!   token of each field is handed to the (un)marshaller unchanged.

#![allow(dead_code)]

use std::sync::{Mutex, MutexGuard};
use xid::{Error, ID};

// ---------------------------------------------------------------------------
// Go's sequential test execution
// ---------------------------------------------------------------------------

/// `go test` runs the tests of a package one after another, so `TestNew` can
/// assert that the shared counter advances by exactly one between two calls.
/// Rust runs integration tests in parallel threads, which would let another
/// test's `New()` slip in between. Every test that generates ids takes this
/// lock, restoring the Go execution model for the generator only.
static GEN_LOCK: Mutex<()> = Mutex::new(());

/// Serialises id generation across the tests of one test binary.
pub fn gen_lock() -> MutexGuard<'static, ()> {
    GEN_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ---------------------------------------------------------------------------
// math/rand stand-in
// ---------------------------------------------------------------------------

/// Deterministic xorshift64* PRNG, standing in for Go's global `math/rand`.
pub struct Rand {
    state: u64,
}

impl Default for Rand {
    fn default() -> Self {
        Rand::new(0x9E3779B97F4A7C15)
    }
}

impl Rand {
    pub fn new(seed: u64) -> Self {
        Rand {
            state: if seed == 0 { 1 } else { seed },
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }

    /// Go's `rand.Intn(n)`.
    pub fn intn(&mut self, n: usize) -> usize {
        assert!(n > 0, "invalid argument to Intn");
        (self.next_u64() % n as u64) as usize
    }
}

// ---------------------------------------------------------------------------
// testing/quick stand-in
// ---------------------------------------------------------------------------

/// Runs `f` `max_count` times over generated `(ID, byte)` pairs, like
/// `quick.Check(f, &quick.Config{Values: …, MaxCount: …})`.
///
/// Returns `Err(message)` on the first falsifying input, mirroring the
/// `*quick.CheckError` Go reports.
pub fn quick_check<F, G>(max_count: usize, mut values: G, f: F) -> Result<(), String>
where
    G: FnMut(&mut Rand) -> (ID, u8),
    F: Fn(ID, u8) -> bool,
{
    let mut r = Rand::default();
    for i in 0..max_count {
        let (id, c) = values(&mut r);
        if !f(id, c) {
            return Err(format!("#{}: failed on input {}, {}", i + 1, id, c as char));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// encoding/json stand-in
// ---------------------------------------------------------------------------

/// Port of the Go test's
/// `type jsonType struct { ID *ID; Str string }`.
#[derive(Debug, Default, PartialEq)]
pub struct JsonType {
    pub id: Option<ID>,
    pub str: String,
}

/// `json.Marshal(&v)` for [`JsonType`]: field order is declaration order and a
/// `*ID` is rendered through `MarshalJSON`.
pub fn json_marshal(v: &JsonType) -> Result<String, Error> {
    let id = match &v.id {
        // encoding/json emits `null` for a nil pointer without calling the
        // marshaller.
        None => "null".to_string(),
        Some(id) => String::from_utf8(id.marshal_json()?).unwrap(),
    };
    Ok(format!("{{\"ID\":{},\"Str\":{}}}", id, json_quote(&v.str)))
}

/// `json.Unmarshal(data, &v)` for [`JsonType`].
///
/// Like `encoding/json`, the raw token of the `ID` field is passed to
/// `UnmarshalJSON` verbatim, so a non-string token (e.g. `1`) reaches the
/// method and produces the package's own error.
pub fn json_unmarshal(data: &[u8], v: &mut JsonType) -> Result<(), Error> {
    for (key, raw) in json_object_fields(data) {
        match key.as_str() {
            "ID" => {
                let mut id = v.id.take().unwrap_or_default();
                id.unmarshal_json(&raw)?;
                v.id = Some(id);
            }
            "Str" if raw.first() == Some(&b'"') => {
                v.str = json_unquote(&raw);
            }
            _ => {}
        }
    }
    Ok(())
}

fn json_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn json_unquote(raw: &[u8]) -> String {
    let s = String::from_utf8_lossy(raw);
    let s = s.trim_matches('"');
    s.replace("\\\"", "\"").replace("\\\\", "\\")
}

/// Splits a flat JSON object into `(key, raw value token)` pairs.
fn json_object_fields(data: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut i = 0usize;

    let skip_ws = |i: &mut usize| {
        while *i < data.len() && (data[*i] as char).is_whitespace() {
            *i += 1;
        }
    };

    skip_ws(&mut i);
    if i >= data.len() || data[i] != b'{' {
        return out;
    }
    i += 1;

    loop {
        skip_ws(&mut i);
        if i >= data.len() || data[i] == b'}' {
            break;
        }
        // key
        let key_start = i;
        i = scan_value(data, i);
        let key = json_unquote(&data[key_start..i]);
        skip_ws(&mut i);
        if i >= data.len() || data[i] != b':' {
            break;
        }
        i += 1;
        skip_ws(&mut i);
        // value
        let val_start = i;
        i = scan_value(data, i);
        out.push((key, data[val_start..i].to_vec()));
        skip_ws(&mut i);
        if i < data.len() && data[i] == b',' {
            i += 1;
            continue;
        }
        break;
    }
    out
}

/// Returns the index just past the JSON value starting at `i`.
fn scan_value(data: &[u8], mut i: usize) -> usize {
    if i >= data.len() {
        return i;
    }
    match data[i] {
        b'"' => {
            i += 1;
            while i < data.len() {
                match data[i] {
                    b'\\' => i += 2,
                    b'"' => return i + 1,
                    _ => i += 1,
                }
            }
            i
        }
        b'{' | b'[' => {
            let (open, close) = if data[i] == b'{' {
                (b'{', b'}')
            } else {
                (b'[', b']')
            };
            let mut depth = 0usize;
            while i < data.len() {
                match data[i] {
                    b'"' => i = scan_value(data, i),
                    c if c == open => {
                        depth += 1;
                        i += 1;
                    }
                    c if c == close => {
                        depth -= 1;
                        i += 1;
                        if depth == 0 {
                            return i;
                        }
                    }
                    _ => i += 1,
                }
            }
            i
        }
        _ => {
            while i < data.len()
                && !matches!(data[i], b',' | b'}' | b']')
                && !(data[i] as char).is_whitespace()
            {
                i += 1;
            }
            i
        }
    }
}
