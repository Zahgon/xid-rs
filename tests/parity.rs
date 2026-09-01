//! Byte-for-byte parity with the Go implementation.
//!
//! The two fixtures under `tests/testdata/` were produced by *running*
//! `github.com/rs/xid` (the generator is embedded in
//! `examples/parity_oracle.rs`; `cargo run --example parity_oracle` regenerates
//! them and diffs), not by reading its source. Every observable output of
//! every exported method is recorded there, so this file is the gate that
//! proves the port did not drift.
//!
//! * `encode_golden.tsv` — 496 ids: the three fixtures from `id_test.go`,
//!   all-zero, all-`0xFF`, a single-bit sweep over all 96 bits, the eight
//!   boundary byte values in each of the 12 positions, and 300 deterministic
//!   random ids. Columns: raw hex, `String()`, `Time().Unix()`, `Machine()`,
//!   `Pid()`, `Counter()`, `MarshalJSON()`, `MarshalText()`, `Value()`,
//!   `IsNil()`, `xidb.ID.Value()`.
//! * `decode_golden.tsv` — 2,045 inputs: every one of the 256 byte values at
//!   the five positions that drive the length, alphabet and canonical-tail
//!   checks, full last-character sweeps over 20 random ids, every length from
//!   0 to 25, plus the literals from the Go test-suite. Columns: hex of the
//!   input bytes, then `ok:<hex>` or `err:<message>:<hex>`.

use std::time::{Duration, UNIX_EPOCH};
use xid::{b, from_bytes, from_string, DriverValue, Error, ID};

fn unhex(s: &str) -> Vec<u8> {
    assert!(s.len() % 2 == 0, "odd hex string: {s:?}");
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[test]
fn encode_matches_go() {
    let data = include_str!("testdata/encode_golden.tsv");
    let mut n = 0;

    for (lineno, line) in data.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        assert_eq!(f.len(), 11, "line {}: {line:?}", lineno + 1);

        let raw = unhex(f[0]);
        let id = from_bytes(&raw).expect("FromBytes");
        let ctx = format!("line {} ({})", lineno + 1, f[0]);

        // String() / Display / MarshalText / Encode all agree with Go.
        assert_eq!(id.string(), f[1], "String() {ctx}");
        assert_eq!(format!("{id}"), f[1], "Display {ctx}");
        assert_eq!(
            String::from_utf8(id.marshal_text().unwrap()).unwrap(),
            f[7],
            "MarshalText() {ctx}"
        );
        let mut dst = [0u8; 20];
        assert_eq!(
            std::str::from_utf8(id.encode(&mut dst)).unwrap(),
            f[1],
            "Encode() {ctx}"
        );

        // Component accessors.
        let secs: i64 = f[2].parse().unwrap();
        assert_eq!(
            id.time(),
            UNIX_EPOCH + Duration::from_secs(secs as u64),
            "Time() {ctx}"
        );
        assert_eq!(id.unix_seconds(), secs, "Time().Unix() {ctx}");
        assert_eq!(hex(id.machine()), f[3], "Machine() {ctx}");
        assert_eq!(id.pid().to_string(), f[4], "Pid() {ctx}");
        assert_eq!(id.counter().to_string(), f[5], "Counter() {ctx}");

        // MarshalJSON, including the `null` a nil id produces.
        assert_eq!(
            String::from_utf8(id.marshal_json().unwrap()).unwrap(),
            f[6],
            "MarshalJSON() {ctx}"
        );

        // Value(): a string, or SQL NULL for the nil id.
        let want_value = if f[8] == "<nil>" {
            DriverValue::Null
        } else {
            DriverValue::Str(f[8].to_string())
        };
        assert_eq!(id.value().unwrap(), want_value, "Value() {ctx}");

        // IsNil / IsZero.
        let want_nil: bool = f[9].parse().unwrap();
        assert_eq!(id.is_nil(), want_nil, "IsNil() {ctx}");
        assert_eq!(id.is_zero(), want_nil, "IsZero() {ctx}");

        // The `b` sub-package's Value(): raw bytes, or SQL NULL.
        let want_bvalue = if f[10] == "<nil>" {
            DriverValue::Null
        } else {
            DriverValue::Bytes(unhex(f[10]))
        };
        assert_eq!(
            b::ID { id }.value().unwrap(),
            want_bvalue,
            "xidb.Value() {ctx}"
        );

        // Round-trip: the string Go produced decodes back to the same bytes.
        assert_eq!(from_string(f[1]).unwrap(), id, "round-trip {ctx}");
        // …and so does a Scan of both representations.
        let mut scanned = ID::default();
        scanned.scan(f[1]).unwrap();
        assert_eq!(scanned, id, "Scan(text) {ctx}");
        let mut scanned = ID::default();
        scanned.scan(&raw[..]).unwrap();
        assert_eq!(scanned, id, "Scan(raw) {ctx}");
        // …and through the `b` sub-package.
        let mut bscanned = b::ID::default();
        bscanned.scan(&raw[..]).unwrap();
        assert_eq!(bscanned.id, id, "xidb.Scan(raw) {ctx}");

        n += 1;
    }

    assert!(n >= 496, "expected the full corpus, got {n} rows");
}

#[test]
fn decode_matches_go() {
    let data = include_str!("testdata/decode_golden.tsv");
    let mut n = 0;
    let mut ok_count = 0;
    let mut err_count = 0;

    for (lineno, line) in data.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let (input_hex, result) = line.split_once('\t').expect("two columns");
        let input = unhex(input_hex);
        let ctx = format!("line {} (input {input_hex})", lineno + 1);

        // Go: `FromString(string(b))` — a Go string is just bytes, so the port
        // is exercised at the byte level through UnmarshalText, which is what
        // FromString calls.
        let mut got = ID::default();
        let res = got.unmarshal_text(&input);

        if let Some(rest) = result.strip_prefix("ok:") {
            assert!(res.is_ok(), "{ctx}: want ok, got {:?}", res.unwrap_err());
            assert_eq!(hex(got.bytes()), rest, "{ctx}: decoded bytes");
            ok_count += 1;
        } else {
            let rest = result.strip_prefix("err:").expect("ok:/err: prefix");
            let (msg, want_hex) = rest.rsplit_once(':').expect("message:hex");
            let err = res.expect_err(&format!("{ctx}: want error"));
            assert_eq!(err.to_string(), msg, "{ctx}: error message");
            assert_eq!(err, Error::InvalidId, "{ctx}: error value");
            assert_eq!(hex(got.bytes()), want_hex, "{ctx}: id after error");
            err_count += 1;
        }

        // When the input is valid UTF-8, `from_string` must agree with the
        // byte-level path (this is the exact Go call site).
        if let Ok(s) = std::str::from_utf8(&input) {
            match from_string(s) {
                Ok(id) => {
                    assert!(result.starts_with("ok:"), "{ctx}: from_string ok");
                    assert_eq!(id, got, "{ctx}: from_string value");
                }
                Err(e) => {
                    assert!(result.starts_with("err:"), "{ctx}: from_string err");
                    assert_eq!(e, Error::InvalidId, "{ctx}: from_string error");
                }
            }
            // `str::parse` is the same entry point.
            assert_eq!(
                s.parse::<ID>().is_ok(),
                result.starts_with("ok:"),
                "{ctx}: parse()"
            );
        }

        n += 1;
    }

    assert!(n >= 2045, "expected the full corpus, got {n} rows");
    assert!(ok_count > 100, "corpus should contain valid ids");
    assert!(err_count > 900, "corpus should contain invalid ids");
}

/// `NewWithTime` must encode a timestamp exactly like Go's
/// `binary.BigEndian.PutUint32(id[:], uint32(t.Unix()))`.
///
/// The Go test-suite never calls `NewWithTime`, so the expected values below
/// were produced by running it: `t.Unix()` truncates towards negative infinity
/// and the `uint32` conversion wraps, which together cover pre-epoch times and
/// times past 2106.
#[test]
fn new_with_time_matches_go() {
    // (Go's time.Unix(sec, nsec) arguments, t.Unix(), first 4 bytes, Time().Unix())
    let cases: &[(i64, i64, i64, &str, i64)] = &[
        (0, 0, 0, "00000000", 0),
        (1, 0, 1, "00000001", 1),
        (1300816219, 0, 1300816219, "4d88e15b", 1300816219),
        (1300816219, 999999999, 1300816219, "4d88e15b", 1300816219),
        (-1, 0, -1, "ffffffff", 4294967295),
        (0, -500000000, -1, "ffffffff", 4294967295),
        (-1, -1, -2, "fffffffe", 4294967294),
        (4294967296, 0, 4294967296, "00000000", 0),
        (4294967301, 0, 4294967301, "00000005", 5),
        (4294967295, 0, 4294967295, "ffffffff", 4294967295),
        (8589934592, 7, 8589934592, "00000000", 0),
        (-4294967296, 0, -4294967296, "00000000", 0),
        (2147483647, 0, 2147483647, "7fffffff", 2147483647),
        (4294967295, 999999999, 4294967295, "ffffffff", 4294967295),
    ];

    for &(sec, nsec, want_unix, want_prefix, want_time_unix) in cases {
        let t = go_time_unix(sec, nsec);
        let id = xid::new_with_time(t);
        let ctx = format!("time.Unix({sec}, {nsec}) [Unix()={want_unix}]");
        assert_eq!(hex(&id.bytes()[..4]), want_prefix, "timestamp bytes {ctx}");
        assert_eq!(id.unix_seconds(), want_time_unix, "Time().Unix() {ctx}");
        assert_eq!(
            id.time(),
            UNIX_EPOCH + Duration::from_secs(want_time_unix as u64),
            "Time() {ctx}"
        );
        // The rest of the id is the live machine/pid/counter, as in Go.
        assert_eq!(id.machine(), xid::new().machine());
    }
}

/// Go's `time.Unix(sec, nsec)`: normalises `nsec` into `[0, 1e9)` first.
fn go_time_unix(sec: i64, nsec: i64) -> std::time::SystemTime {
    let mut sec = sec;
    let mut nsec = nsec;
    if !(0..1_000_000_000).contains(&nsec) {
        let n = nsec.div_euclid(1_000_000_000);
        sec += n;
        nsec -= n * 1_000_000_000;
    }
    if sec >= 0 {
        UNIX_EPOCH + Duration::new(sec as u64, nsec as u32)
    } else {
        UNIX_EPOCH - Duration::new(sec.unsigned_abs(), 0) + Duration::from_nanos(nsec as u64)
    }
}

/// `Sort` must order ids exactly like Go's `sort.Sort(sorter(ids))`.
///
/// Go, on the fixtures from `id_test.go`, prints:
/// `00000000000000000000,0000005anf6drrg0000g,9m4e2mr0ui3e8a215n4g`.
#[test]
fn sort_matches_go() {
    let mut ids = vec![
        ID([
            0x4d, 0x88, 0xe1, 0x5b, 0x60, 0xf4, 0x86, 0xe4, 0x28, 0x41, 0x2d, 0xc9,
        ]),
        ID([0u8; 12]),
        ID([
            0x00, 0x00, 0x00, 0x00, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0x00, 0x00, 0x01,
        ]),
    ];
    xid::sort(&mut ids);
    let got: Vec<String> = ids.iter().map(|i| i.string()).collect();
    assert_eq!(
        got.join(","),
        "00000000000000000000,0000005anf6drrg0000g,9m4e2mr0ui3e8a215n4g"
    );

    // A larger, self-checking case: sorting must equal lexicographic byte order
    // on the raw representation.
    let data = include_str!("testdata/encode_golden.tsv");
    let mut ids: Vec<ID> = data
        .lines()
        .take(400)
        .map(|l| from_bytes(&unhex(l.split('\t').next().unwrap())).unwrap())
        .collect();
    let mut want = ids.clone();
    want.sort_by(|a, b| a.bytes().cmp(b.bytes()));
    xid::sort(&mut ids);
    assert_eq!(ids, want);

    // …and the string order matches the byte order (the "K-ordered" property).
    let strings: Vec<String> = ids.iter().map(|i| i.string()).collect();
    let mut sorted_strings = strings.clone();
    sorted_strings.sort();
    assert_eq!(strings, sorted_strings, "base32 hex must preserve ordering");
}
