//! Port of `id_test.go`.
//!
//! Every Go test function has a counterpart with the same name (snake-cased),
//! the same inputs, the same assertions and the same iteration counts.
//! `TestMachineFromEnv` is the one exception: it mutates a process-wide
//! environment variable that the machine id is derived from, so it lives in
//! its own test binary (`tests/machine_env_test.rs`) where nothing runs
//! concurrently with it — the isolation Go gets for free by initialising
//! `machineID` before any test starts.

mod common;

use common::{gen_lock, json_marshal, json_unmarshal, quick_check, JsonType, Rand};
use std::time::{Duration, UNIX_EPOCH};
use xid::{
    from_bytes, from_string, new, nil_id, sort, DriverValue, Error, ScanValue, Sorter, ENCODED_LEN,
    ENCODING, ID,
};

struct IDParts {
    id: ID,
    timestamp: i64,
    machine: &'static [u8],
    pid: u16,
    counter: i32,
}

fn ids() -> Vec<IDParts> {
    vec![
        IDParts {
            id: ID([
                0x4d, 0x88, 0xe1, 0x5b, 0x60, 0xf4, 0x86, 0xe4, 0x28, 0x41, 0x2d, 0xc9,
            ]),
            timestamp: 1300816219,
            machine: &[0x60, 0xf4, 0x86],
            pid: 0xe428,
            counter: 4271561,
        },
        IDParts {
            id: ID([
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            ]),
            timestamp: 0,
            machine: &[0x00, 0x00, 0x00],
            pid: 0x0000,
            counter: 0,
        },
        IDParts {
            id: ID([
                0x00, 0x00, 0x00, 0x00, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0x00, 0x00, 0x01,
            ]),
            timestamp: 0,
            machine: &[0xaa, 0xbb, 0xcc],
            pid: 0xddee,
            counter: 1,
        },
    ]
}

/// The three fixture ids, in the order `id_test.go` declares them.
fn id_list() -> Vec<ID> {
    ids().into_iter().map(|p| p.id).collect()
}

fn id_parts_extraction(i: usize) {
    let v = &ids()[i];
    // t.Run(fmt.Sprintf("Test%d", i), ...)
    let name = format!("Test{i}");
    let want_time = UNIX_EPOCH + Duration::from_secs(v.timestamp as u64);
    assert_eq!(v.id.time(), want_time, "{name}: Time()");
    assert_eq!(v.id.machine(), v.machine, "{name}: Machine()");
    assert_eq!(v.id.pid(), v.pid, "{name}: Pid()");
    assert_eq!(v.id.counter(), v.counter, "{name}: Counter()");
}

// Go's `t.Run` subtests are separate run units that report and fail
// independently. Rust has no subtests, so each one becomes its own `#[test]`
// carrying the Go subtest's name — `TestIDPartsExtraction/Test0` and the two
// that follow it.

#[test]
fn test_id_parts_extraction_test0() {
    id_parts_extraction(0);
}

#[test]
fn test_id_parts_extraction_test1() {
    id_parts_extraction(1);
}

#[test]
fn test_id_parts_extraction_test2() {
    id_parts_extraction(2);
}

#[test]
fn test_padding() {
    let mut rand = Rand::default();
    for _ in 0..100_000 {
        let mut want_bytes = [0u8; 20];
        want_bytes[19] = ENCODING[0]; // 0
        want_bytes[0..13].copy_from_slice(b"c6e52g2mrqcjl"); // c6e52g2mrqcjl44hf170
        for j in 0..6 {
            want_bytes[13 + j] = ENCODING[rand.intn(32)];
        }
        let want = String::from_utf8(want_bytes.to_vec()).unwrap();
        let id = from_string(&want).unwrap_or_default();
        let got = id.string();
        assert_eq!(got, want, "String() = {got}, want {want} {want_bytes:?}");
    }
}

#[test]
fn test_new() {
    let _guard = gen_lock();
    // Generate 10 ids
    let ids: Vec<ID> = (0..10).map(|_| new()).collect();
    for i in 1..10 {
        let prev_id = ids[i - 1];
        let id = ids[i];
        // Test for uniqueness among all other 9 generated ids
        for (j, tid) in ids.iter().enumerate() {
            if j != i {
                assert!(id.compare(tid) != 0, "generated ID is not unique ({i}/{j})");
            }
        }
        // Check that timestamp was incremented and is within 30 seconds of the previous one
        let secs = id
            .time()
            .duration_since(prev_id.time())
            .map(|d| d.as_secs_f64())
            .unwrap_or(-1.0);
        assert!(
            (0.0..=30.0).contains(&secs),
            "wrong timestamp in generated ID"
        );
        // Check that machine ids are the same
        assert_eq!(id.machine(), prev_id.machine(), "machine ID not equal");
        // Check that pids are the same
        assert_eq!(id.pid(), prev_id.pid(), "pid not equal");
        // Test for proper increment
        let got = id.counter().wrapping_sub(prev_id.counter());
        assert_eq!(
            got, 1,
            "wrong increment in generated ID, delta={got}, want 1"
        );
    }
}

#[test]
fn test_id_string() {
    let id = ID([
        0x4d, 0x88, 0xe1, 0x5b, 0x60, 0xf4, 0x86, 0xe4, 0x28, 0x41, 0x2d, 0xc9,
    ]);
    assert_eq!(id.string(), "9m4e2mr0ui3e8a215n4g");
    // Go's %v/%s go through String(); so does Display here.
    assert_eq!(format!("{id}"), "9m4e2mr0ui3e8a215n4g");
}

#[test]
fn test_id_encode() {
    let id = ID([
        0x4d, 0x88, 0xe1, 0x5b, 0x60, 0xf4, 0x86, 0xe4, 0x28, 0x41, 0x2d, 0xc9,
    ]);
    let mut text = vec![0u8; ENCODED_LEN];
    let got = String::from_utf8(id.encode(&mut text).to_vec()).unwrap();
    assert_eq!(got, "9m4e2mr0ui3e8a215n4g");
}

#[test]
fn test_from_string() {
    let got = from_string("9m4e2mr0ui3e8a215n4g").expect("FromString");
    let want = ID([
        0x4d, 0x88, 0xe1, 0x5b, 0x60, 0xf4, 0x86, 0xe4, 0x28, 0x41, 0x2d, 0xc9,
    ]);
    assert_eq!(got, want);
}

#[test]
fn test_from_string_invalid() {
    let err = from_string("invalid").unwrap_err();
    assert_eq!(err, Error::InvalidId, "FromString(invalid) err={err}");
    // Well-formed alphabet, but the trailing bits do not round-trip: the id
    // must come back as nilID.
    let res = from_string("c6e52g2mrqcjl44hf179");
    let id = res.clone().unwrap_or_default();
    assert_eq!(id, nil_id(), "FromString() ={id}, want {}", nil_id());
    // Go's test ignores this error; assert it too, so a decoder that silently
    // accepted the non-canonical tail could not pass by returning a nil id.
    assert_eq!(res.unwrap_err(), Error::InvalidId);
}

/// Ports `TestIDJSONMarshaling`.
#[test]
fn test_idjson_marshaling() {
    let id = ID([
        0x4d, 0x88, 0xe1, 0x5b, 0x60, 0xf4, 0x86, 0xe4, 0x28, 0x41, 0x2d, 0xc9,
    ]);
    let v = JsonType {
        id: Some(id),
        str: "test".to_string(),
    };
    let data = json_marshal(&v).expect("json.Marshal");
    assert_eq!(data, r#"{"ID":"9m4e2mr0ui3e8a215n4g","Str":"test"}"#);
}

/// Ports `TestIDJSONUnmarshaling`.
#[test]
fn test_idjson_unmarshaling() {
    let data = br#"{"ID":"9m4e2mr0ui3e8a215n4g","Str":"test"}"#;
    let mut v = JsonType::default();
    json_unmarshal(data, &mut v).expect("json.Unmarshal");
    let want = ID([
        0x4d, 0x88, 0xe1, 0x5b, 0x60, 0xf4, 0x86, 0xe4, 0x28, 0x41, 0x2d, 0xc9,
    ]);
    let got = v.id.expect("ID field");
    assert_eq!(
        got.compare(&want),
        0,
        "json.Unmarshal() = {got}, want {want}"
    );
    assert_eq!(v.str, "test");
}

/// Ports `TestIDJSONUnmarshalingError`.
#[test]
fn test_idjson_unmarshaling_error() {
    let mut v = JsonType::default();
    for data in [
        &br#"{"ID":"9M4E2MR0UI3E8A215N4G"}"#[..],
        &br#"{"ID":"TYjhW2D0huQoQS"}"#[..],
        &br#"{"ID":"TYjhW2D0huQoQS3kdk"}"#[..],
        &br#"{"ID":1}"#[..],
    ] {
        let err = json_unmarshal(data, &mut v).unwrap_err();
        assert_eq!(
            err,
            Error::InvalidId,
            "json.Unmarshal() err={err}, want {}",
            Error::InvalidId
        );
    }
}

#[test]
fn test_id_driver_value() {
    let id = ID([
        0x4d, 0x88, 0xe1, 0x5b, 0x60, 0xf4, 0x86, 0xe4, 0x28, 0x41, 0x2d, 0xc9,
    ]);
    let got = id.value().expect("Value()");
    assert_eq!(got, DriverValue::Str("9m4e2mr0ui3e8a215n4g".to_string()));
    assert_eq!(got, "9m4e2mr0ui3e8a215n4g");

    // A nil id yields SQL NULL.
    assert_eq!(nil_id().value().unwrap(), DriverValue::Null);
}

#[test]
fn test_id_driver_scan() {
    let mut got = ID::default();
    got.scan("9m4e2mr0ui3e8a215n4g").expect("Scan()");
    let want = ID([
        0x4d, 0x88, 0xe1, 0x5b, 0x60, 0xf4, 0x86, 0xe4, 0x28, 0x41, 0x2d, 0xc9,
    ]);
    assert_eq!(got.compare(&want), 0, "Scan() = {got}, want {want}");
}

#[test]
fn test_id_driver_scan_error() {
    let mut id = ID::default();
    let got = id.scan(0).unwrap_err();
    assert_eq!(
        got.to_string(),
        "xid: scanning unsupported type: int",
        "Scan() err={got}"
    );
    let got = id.scan("0").unwrap_err();
    assert_eq!(got, Error::InvalidId, "Scan() err={got}");
}

#[test]
fn test_id_driver_scan_byte_from_database() {
    let mut got = ID::default();
    let bs = b"9m4e2mr0ui3e8a215n4g";
    got.scan(&bs[..]).expect("Scan()");
    let want = ID([
        0x4d, 0x88, 0xe1, 0x5b, 0x60, 0xf4, 0x86, 0xe4, 0x28, 0x41, 0x2d, 0xc9,
    ]);
    assert_eq!(got.compare(&want), 0, "Scan() = {got}, want {want}");
}

#[test]
fn test_id_driver_scan_raw_bytes() {
    let want = ID([
        0x4d, 0x88, 0xe1, 0x5b, 0x60, 0xf4, 0x86, 0xe4, 0x28, 0x41, 0x2d, 0xc9,
    ]);
    let mut got = ID::default();
    got.scan(want.bytes()).expect("Scan(raw)");
    assert_eq!(got.compare(&want), 0, "Scan(raw) = {got}, want {want}");
}

#[test]
fn test_id_driver_scan_nil() {
    // `case nil:` of the Go switch.
    let mut got = ID([
        0x4d, 0x88, 0xe1, 0x5b, 0x60, 0xf4, 0x86, 0xe4, 0x28, 0x41, 0x2d, 0xc9,
    ]);
    got.scan(ScanValue::Nil).expect("Scan(nil)");
    assert_eq!(got, nil_id());
}

#[test]
fn test_from_string_quick() {
    let _guard = gen_lock();
    // Mutating any one character of a valid id must never yield the same id
    // without an error.
    let f = |id1: ID, c: u8| -> bool {
        let s1 = id1.string();
        for i in 0..s1.len() {
            let mut s2 = s1.clone().into_bytes();
            s2[i] = c;
            let s2 = String::from_utf8_lossy(&s2).into_owned();
            let res = from_string(&s2);
            let id2 = res.clone().unwrap_or_default();
            if id1 == id2 && res.is_ok() && c != s1.as_bytes()[i] {
                eprintln!(
                    "comparing XIDs:\na: {s1:?}\nb: {s2:?} (index {i} changed to {})",
                    c as char
                );
                return false;
            }
        }
        true
    };
    let values = |r: &mut Rand| -> (ID, u8) {
        let i = r.intn(ENCODING.len());
        (new(), ENCODING[i])
    };
    if let Err(e) = quick_check(1000, values, f) {
        panic!("{e}");
    }
}

#[test]
fn test_from_string_quick_invalid_chars() {
    let _guard = gen_lock();
    let f = |id1: ID, c: u8| -> bool {
        let s1 = id1.string();
        for i in 0..s1.len() {
            let mut s2 = s1.clone().into_bytes();
            s2[i] = c;
            // Go builds a string from arbitrary bytes; FromString works on the
            // byte level, so feed the bytes through unmarshal_text directly to
            // keep non-UTF-8 inputs intact.
            let mut id2 = ID::default();
            let res = id2.unmarshal_text(&s2);
            if res.is_err() {
                id2 = ID::default();
            }
            if id1 == id2 && res.is_ok() && c != s1.as_bytes()[i] {
                eprintln!(
                    "comparing XIDs:\na: {s1:?}\nb: {:?} (index {i} changed to {})",
                    String::from_utf8_lossy(&s2),
                    c as char
                );
                return false;
            }
        }
        true
    };
    let values = |r: &mut Rand| -> (ID, u8) {
        let i = r.intn(0xFF);
        (new(), i as u8)
    };
    if let Err(e) = quick_check(2000, values, f) {
        panic!("{e}");
    }
}

fn id_is_nil(name: &str, id: ID, want: bool) {
    assert_eq!(id.is_nil(), want, "{name}: IsNil()");
    // IsZero is an alias of IsNil.
    assert_eq!(id.is_zero(), want, "{name}: IsZero()");
}

/// `TestID_IsNil/ID_not_nil`
#[test]
fn test_id_is_nil_id_not_nil() {
    let _guard = gen_lock();
    id_is_nil("ID not nil", new(), false);
}

/// `TestID_IsNil/Nil_ID`
#[test]
fn test_id_is_nil_nil_id() {
    id_is_nil("Nil ID", ID::default(), true);
}

#[test]
fn test_nil_id() {
    let got = ID::default();
    assert_eq!(got, nil_id(), "NilID() not equal ID{{}}");
}

#[test]
fn test_nil_id_is_nil() {
    assert!(nil_id().is_nil(), "NilID().IsNil() is not true");
}

#[test]
fn test_from_bytes_invariant() {
    let _guard = gen_lock();
    let want = new();
    let got = from_bytes(want.bytes()).expect("FromBytes");
    assert_eq!(got.compare(&want), 0, "FromBytes(id.Bytes()) != id");
}

#[test]
fn test_from_bytes_invalid_bytes() {
    let cases = [(11usize, true), (12, false), (13, true)];
    for (length, should_fail) in cases {
        let b = vec![0u8; length];
        let err = from_bytes(&b).err();
        assert_eq!(
            err.is_some(),
            should_fail,
            "FromBytes() error got {}, want {should_fail}",
            err.is_some()
        );
        if should_fail {
            assert_eq!(err.unwrap(), Error::InvalidId);
        }
    }
}

#[test]
fn test_id_compare() {
    let ids = id_list();
    let pairs: Vec<(ID, ID, i32)> = vec![
        (ids[1], ids[0], -1),
        (
            ID([
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            ]),
            ids[2],
            -1,
        ),
        (ids[0], ids[0], 0),
    ];
    for (left, right, expected) in pairs {
        assert_eq!(
            expected,
            left.compare(&right),
            "{left} Compare to {right} should return {expected}"
        );
        assert_eq!(
            -expected,
            right.compare(&left),
            "{right} Compare to {left} should return {}",
            -expected
        );
    }
}

#[test]
fn test_sorter_len() {
    let mut empty: Vec<ID> = Vec::new();
    assert_eq!(Sorter(&mut empty).len(), 0, "Len()");
    let mut list = id_list();
    assert_eq!(Sorter(&mut list).len(), 3, "Len()");
}

#[test]
fn test_sorter_less() {
    let mut list = id_list();
    let sorter = Sorter(&mut list);
    assert!(sorter.less(1, 0), "Less(1, 0) not true");
    assert!(!sorter.less(2, 1), "Less(2, 1) true");
    assert!(!sorter.less(0, 0), "Less(0, 0) true");
}

#[test]
fn test_sorter_swap() {
    let id_list = id_list();
    let mut ids: Vec<ID> = Vec::new();
    ids.extend_from_slice(&id_list);
    {
        let mut sorter = Sorter(&mut ids);
        sorter.swap(0, 1);
        sorter.swap(2, 2);
    }
    assert_eq!(ids[0], id_list[1], "ids[0] != IDList[1]");
    assert_eq!(ids[1], id_list[0], "ids[1] != IDList[0]");
    assert_eq!(ids[2], id_list[2], "ids[2], IDList[2]");
}

#[test]
fn test_sort() {
    let id_list = id_list();
    let mut ids: Vec<ID> = Vec::new();
    ids.extend_from_slice(&id_list);
    sort(&mut ids);
    assert_eq!(ids, vec![id_list[1], id_list[2], id_list[0]]);
}
