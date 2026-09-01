//! Exercises the whole public surface of the crate.
//!
//! `id_test.go` reaches the Go API through package-level functions and
//! methods; the Rust port additionally exposes the idiomatic trait
//! implementations (`FromStr`, `TryFrom`, `Index`, `Deref`, …) that stand in
//! for Go's syntax. Those need coverage too, otherwise a broken conversion
//! could ship untested.

mod common;

use common::gen_lock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use xid::{
    b, driver::ScanValue, from_bytes, from_string, gosha256, new, new_with_time, nil_id,
    DriverValue, Error, Sorter, ENCODED_LEN, ENCODING, ERR_INVALID_ID, ID, RAW_LEN,
};

const FIXTURE: ID = ID([
    0x4d, 0x88, 0xe1, 0x5b, 0x60, 0xf4, 0x86, 0xe4, 0x28, 0x41, 0x2d, 0xc9,
]);
const FIXTURE_STR: &str = "9m4e2mr0ui3e8a215n4g";

#[test]
fn constants_match_go() {
    assert_eq!(ENCODED_LEN, 20);
    assert_eq!(RAW_LEN, 12);
    assert_eq!(ENCODING, b"0123456789abcdefghijklmnopqrstuv");
    assert_eq!(ENCODING.len(), 32);
    assert_eq!(ERR_INVALID_ID, Error::InvalidId);
    assert_eq!(ERR_INVALID_ID.to_string(), "xid: invalid ID");
    // Go's `strErr.Error()`.
    assert_eq!(ERR_INVALID_ID.error(), "xid: invalid ID");
    assert_eq!(
        Error::ScanUnsupportedType("float64".into()).error(),
        "xid: scanning unsupported type: float64"
    );
    // std::error::Error is implemented.
    let e: &dyn std::error::Error = &ERR_INVALID_ID;
    assert_eq!(e.to_string(), "xid: invalid ID");
    assert_eq!(format!("{:?}", Error::InvalidId), "InvalidId");
}

#[test]
fn associated_constructors_match_free_functions() {
    let _guard = gen_lock();

    // Every package-level Go function is also reachable as an associated fn.
    assert_eq!(ID::from_string(FIXTURE_STR).unwrap(), FIXTURE);
    assert_eq!(ID::from_bytes(FIXTURE.bytes()).unwrap(), FIXTURE);
    assert_eq!(ID::nil(), nil_id());
    assert_eq!(ID::default(), nil_id());

    let a = ID::new();
    let b = new();
    assert_eq!(b.counter().wrapping_sub(a.counter()), 1);

    let t = UNIX_EPOCH + Duration::from_secs(1300816219);
    assert_eq!(ID::new_with_time(t).unix_seconds(), 1300816219);
    assert_eq!(new_with_time(t).unix_seconds(), 1300816219);
    assert_eq!(ID::new_with_time(t).time(), t);

    // Freshly generated ids carry "now".
    let now = SystemTime::now();
    let delta = new()
        .time()
        .duration_since(now.checked_sub(Duration::from_secs(2)).unwrap())
        .unwrap();
    assert!(delta <= Duration::from_secs(5));
}

#[test]
fn conversion_traits() {
    // FromStr / parse
    let id: ID = FIXTURE_STR.parse().unwrap();
    assert_eq!(id, FIXTURE);
    assert_eq!("nope".parse::<ID>().unwrap_err(), Error::InvalidId);

    // TryFrom<&[u8]>
    let id = ID::try_from(FIXTURE.bytes()).unwrap();
    assert_eq!(id, FIXTURE);
    assert_eq!(ID::try_from(&[0u8; 5][..]).unwrap_err(), Error::InvalidId);

    // From<[u8; 12]>
    let raw: [u8; RAW_LEN] = [
        0x4d, 0x88, 0xe1, 0x5b, 0x60, 0xf4, 0x86, 0xe4, 0x28, 0x41, 0x2d, 0xc9,
    ];
    assert_eq!(ID::from(raw), FIXTURE);
    let id2: ID = raw.into();
    assert_eq!(id2, FIXTURE);

    // AsRef<[u8]>
    fn takes_bytes<T: AsRef<[u8]>>(v: T) -> usize {
        v.as_ref().len()
    }
    assert_eq!(takes_bytes(FIXTURE), 12);
    assert_eq!(FIXTURE.as_ref(), FIXTURE.bytes());

    // Index / IndexMut — Go writes `id[4] = machineID[0]`.
    assert_eq!(FIXTURE[0], 0x4d);
    assert_eq!(FIXTURE[11], 0xc9);
    let mut m = FIXTURE;
    m[0] = 0x00;
    assert_eq!(m[0], 0x00);
    assert_ne!(m, FIXTURE);

    // Debug renders like Go's %v (which goes through String()).
    assert_eq!(format!("{FIXTURE:?}"), FIXTURE_STR);
    assert_eq!(format!("{FIXTURE}"), FIXTURE_STR);
    assert_eq!(FIXTURE.to_string(), FIXTURE_STR);
}

#[test]
fn ordering_and_hashing() {
    use std::collections::{BTreeSet, HashMap};

    let a = ID([0u8; 12]);
    let b = FIXTURE;
    assert!(a < b);
    assert_eq!(a.cmp(&b), std::cmp::Ordering::Less);
    assert_eq!(b.compare(&a), 1);
    assert_eq!(a.compare(&a), 0);

    // Ord must agree with Compare on every pair.
    for x in [a, b, ID([0xFF; 12])] {
        for y in [a, b, ID([0xFF; 12])] {
            let want = match x.compare(&y) {
                -1 => std::cmp::Ordering::Less,
                0 => std::cmp::Ordering::Equal,
                _ => std::cmp::Ordering::Greater,
            };
            assert_eq!(x.cmp(&y), want);
        }
    }

    // Usable as a map/set key, like Go's comparable array type.
    let mut set = BTreeSet::new();
    set.insert(b);
    set.insert(a);
    assert_eq!(set.iter().next(), Some(&a));
    let mut map = HashMap::new();
    map.insert(b, "fixture");
    assert_eq!(map.get(&FIXTURE), Some(&"fixture"));
}

#[test]
fn marshal_and_unmarshal_round_trips() {
    // MarshalText -> UnmarshalText
    let text = FIXTURE.marshal_text().unwrap();
    assert_eq!(text, FIXTURE_STR.as_bytes());
    let mut back = ID::default();
    back.unmarshal_text(&text).unwrap();
    assert_eq!(back, FIXTURE);

    // MarshalJSON -> UnmarshalJSON
    let json = FIXTURE.marshal_json().unwrap();
    assert_eq!(json, format!("\"{FIXTURE_STR}\"").as_bytes());
    let mut back = ID::default();
    back.unmarshal_json(&json).unwrap();
    assert_eq!(back, FIXTURE);

    // The nil id marshals to JSON null…
    assert_eq!(nil_id().marshal_json().unwrap(), b"null");
    // …and `null` unmarshals back to the nil id, resetting a non-nil receiver.
    let mut id = FIXTURE;
    id.unmarshal_json(b"null").unwrap();
    assert!(id.is_nil());

    // Short JSON tokens are rejected before UnmarshalText can panic.
    let mut id = ID::default();
    assert_eq!(id.unmarshal_json(b"1").unwrap_err(), Error::InvalidId);
    assert_eq!(id.unmarshal_json(b"").unwrap_err(), Error::InvalidId);
    // A quoted empty string is long enough to reach UnmarshalText, which
    // rejects it on length.
    assert_eq!(id.unmarshal_json(b"\"\"").unwrap_err(), Error::InvalidId);
    // `nul` is not `null`.
    assert_eq!(id.unmarshal_json(b"nul").unwrap_err(), Error::InvalidId);

    // Encode into an oversized buffer: Go writes 20 bytes and returns dst.
    let mut dst = vec![b'.'; 25];
    let out = FIXTURE.encode(&mut dst);
    assert_eq!(out.len(), 25);
    assert_eq!(&out[..20], FIXTURE_STR.as_bytes());
    assert_eq!(&out[20..], b".....");
}

#[test]
#[should_panic]
fn encode_panics_on_short_buffer() {
    // Go panics with "index out of range" for a dst shorter than 20 bytes.
    let mut dst = [0u8; 19];
    FIXTURE.encode(&mut dst);
}

#[test]
fn driver_value_helpers() {
    let v = FIXTURE.value().unwrap();
    assert!(!v.is_null());
    assert_eq!(v.as_str(), Some(FIXTURE_STR));
    assert_eq!(v.as_bytes(), None);
    assert_eq!(v, FIXTURE_STR);
    assert!(v.eq(FIXTURE_STR));

    let n = nil_id().value().unwrap();
    assert!(n.is_null());
    assert_eq!(n.as_str(), None);
    assert_eq!(n.as_bytes(), None);
    assert_eq!(n, DriverValue::Null);

    let bytes = b::ID::new(FIXTURE).value().unwrap();
    assert_eq!(bytes.as_bytes(), Some(FIXTURE.bytes()));
    assert_eq!(bytes.as_str(), None);
    assert!(!bytes.is_null());
    assert!(bytes.eq(FIXTURE.bytes()));
    assert!(b::ID::new(nil_id()).value().unwrap().is_null());
}

#[test]
fn scan_value_covers_the_go_type_names() {
    // Every `%T` the "unsupported type" error can report.
    assert_eq!(ScanValue::Nil.go_type_name(), "<nil>");
    assert_eq!(ScanValue::Str("x").go_type_name(), "string");
    assert_eq!(ScanValue::Bytes(b"x").go_type_name(), "[]uint8");
    assert_eq!(ScanValue::Int(1).go_type_name(), "int");
    assert_eq!(ScanValue::Int64(1).go_type_name(), "int64");
    assert_eq!(ScanValue::Float64(1.0).go_type_name(), "float64");
    assert_eq!(ScanValue::Bool(true).go_type_name(), "bool");

    // …and the conversions that produce them.
    let owned = FIXTURE_STR.to_string();
    let vec = FIXTURE.bytes().to_vec();
    let array: [u8; 12] = FIXTURE.0;
    assert_eq!(ScanValue::from(FIXTURE_STR), ScanValue::Str(FIXTURE_STR));
    assert_eq!(ScanValue::from(&owned), ScanValue::Str(FIXTURE_STR));
    assert_eq!(ScanValue::from(&vec), ScanValue::Bytes(FIXTURE.bytes()));
    assert_eq!(ScanValue::from(&array), ScanValue::Bytes(FIXTURE.bytes()));
    assert_eq!(ScanValue::from(&vec[..]), ScanValue::Bytes(FIXTURE.bytes()));
    assert_eq!(ScanValue::from(1i32), ScanValue::Int(1));
    assert_eq!(ScanValue::from(1i64), ScanValue::Int64(1));
    assert_eq!(ScanValue::from(1.5f64), ScanValue::Float64(1.5));
    assert_eq!(ScanValue::from(true), ScanValue::Bool(true));
    assert_eq!(ScanValue::from(()), ScanValue::Nil);
    assert_eq!(ScanValue::from(None::<i32>), ScanValue::Nil);
    assert_eq!(ScanValue::from(Some(1i32)), ScanValue::Int(1));
    assert_eq!(format!("{:?}", ScanValue::Nil), "Nil");

    // Scan accepts each of those forms.
    let mut id = ID::default();
    id.scan(&owned).unwrap();
    assert_eq!(id, FIXTURE);
    id.scan(&vec).unwrap();
    assert_eq!(id, FIXTURE);
    id.scan(&array).unwrap();
    assert_eq!(id, FIXTURE);
    id.scan(Some(FIXTURE_STR)).unwrap();
    assert_eq!(id, FIXTURE);
    id.scan(None::<&str>).unwrap();
    assert!(id.is_nil());

    // …and rejects the rest with Go's message.
    for (v, want) in [
        (ScanValue::Int64(1), "int64"),
        (ScanValue::Float64(1.0), "float64"),
        (ScanValue::Bool(true), "bool"),
    ] {
        let err = ID::default().scan(v).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!("xid: scanning unsupported type: {want}")
        );
        assert_eq!(err, Error::ScanUnsupportedType(want.to_string()));
    }

    // A []byte that is neither 12 raw bytes nor 20 text bytes.
    let mut id = ID::default();
    assert_eq!(id.scan(&[0u8; 13][..]).unwrap_err(), Error::InvalidId);
}

#[test]
fn sorter_wrapper() {
    let mut empty: Vec<ID> = Vec::new();
    let s = Sorter(&mut empty);
    assert!(s.is_empty());
    assert_eq!(s.len(), 0);

    let mut ids = vec![FIXTURE, ID([0u8; 12])];
    let mut s = Sorter(&mut ids);
    assert!(!s.is_empty());
    assert!(s.less(1, 0));
    s.swap(0, 1);
    assert!(s.less(0, 1));
    assert_eq!(ids[0], ID([0u8; 12]));

    // Sorting an empty or single-element slice is a no-op.
    xid::sort(&mut []);
    let mut one = vec![FIXTURE];
    xid::sort(&mut one);
    assert_eq!(one, vec![FIXTURE]);
}

#[test]
fn b_module_surface() {
    let id = b::ID::new(FIXTURE);
    assert_eq!(id, b::ID::from(FIXTURE));
    assert_eq!(id.id, FIXTURE);
    assert_eq!(ID::from(id), FIXTURE);
    let inner: ID = id.into();
    assert_eq!(inner, FIXTURE);

    // Display/Debug are promoted from the embedded id.
    assert_eq!(id.to_string(), FIXTURE_STR);
    assert_eq!(format!("{id}"), FIXTURE_STR);
    assert_eq!(format!("{id:?}"), FIXTURE_STR);

    // Deref / DerefMut give Go's embedded method set.
    assert_eq!(id.bytes(), FIXTURE.bytes());
    assert_eq!(id.time(), FIXTURE.time());
    let mut mutable = b::ID::default();
    assert!(mutable.is_nil());
    mutable.unmarshal_text(FIXTURE_STR.as_bytes()).unwrap(); // through DerefMut
    assert_eq!(mutable.id, FIXTURE);

    // Ordering and hashing are inherited too.
    assert!(b::ID::default() < id);
    let mut set = std::collections::HashSet::new();
    set.insert(id);
    assert!(set.contains(&b::ID::new(FIXTURE)));

    // Scan of a text-encoded value is *not* supported by the byte wrapper
    // (Go's `b` package only handles []byte and nil).
    let mut x = b::ID::default();
    assert_eq!(
        x.scan(FIXTURE_STR).unwrap_err().to_string(),
        "xid: scanning unsupported type: string"
    );
    // 20 text bytes are the wrong raw length, so FromBytes rejects them.
    assert_eq!(
        x.scan(FIXTURE_STR.as_bytes()).unwrap_err(),
        Error::InvalidId
    );
}

#[test]
fn stdlib_shim_surface() {
    // gosha256::Digest implements Default like Go's zero-value-friendly API.
    let mut d = gosha256::Digest::default();
    d.write(b"abc");
    assert_eq!(d.sum(), gosha256::sum256(b"abc"));

    // goos::hostname is the fallback source for the machine id; on this host it
    // must return a non-empty name.
    let host = xid::goos::hostname().expect("hostname");
    assert!(!host.is_empty(), "hostname should not be empty");

    // goos::getenv mirrors Go's empty-string-for-missing behaviour.
    assert_eq!(xid::goos::getenv("XID_DEFINITELY_NOT_SET_12345"), "");
    std::env::set_var("XID_TEST_GETENV", "hello");
    assert_eq!(xid::goos::getenv("XID_TEST_GETENV"), "hello");
    std::env::remove_var("XID_TEST_GETENV");

    // goos::getpid matches the process id.
    assert_eq!(xid::goos::getpid(), std::process::id() as i64);

    // goos::read_file mirrors os.ReadFile, error included.
    assert!(xid::goos::read_file("/definitely/not/here").is_err());

    // goos::look_path: found in PATH, found by explicit path, not executable,
    // and not found at all.
    assert!(xid::goos::look_path("sh").is_ok());
    assert_eq!(
        xid::goos::look_path("/bin/sh").unwrap(),
        std::path::PathBuf::from("/bin/sh")
    );
    let err = xid::goos::look_path("/etc/hosts").unwrap_err();
    assert!(err.to_string().contains("permission denied"), "{err}");
    let err = xid::goos::look_path("xid-no-such-binary-42").unwrap_err();
    assert!(
        err.to_string()
            .contains("executable file not found in $PATH"),
        "{err}"
    );
}

#[test]
fn from_string_error_leaves_a_nil_id() {
    // Go returns `(ID, error)`; on every error path the id is the zero value.
    for bad in [
        "",
        "invalid",
        "9M4E2MR0UI3E8A215N4G",
        "c6e52g2mrqcjl44hf179",
    ] {
        assert_eq!(from_string(bad).unwrap_or_default(), nil_id(), "{bad:?}");
    }
    // …and FromBytes behaves the same way.
    assert_eq!(from_bytes(&[]).unwrap_or_default(), nil_id());
}
