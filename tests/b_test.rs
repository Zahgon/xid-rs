//! Port of `b/id_test.go` (Go package `xidb`).
//!
//! Go's `t.Run` subtests are separate run units that report and fail
//! independently. Rust has no subtests, so each case of `TestIDValue` and
//! `TestIDScan` becomes its own `#[test]` carrying the Go subtest's name.

use xid::b::ID;
use xid::{from_string, nil_id, DriverValue};

fn fixture() -> xid::ID {
    from_string("9m4e2mr0ui3e8a215n4g").expect("FromString")
}

// ---------------------------------------------------------------------------
// TestIDValue
// ---------------------------------------------------------------------------

fn id_value(name: &str, id: ID, expected_val: DriverValue) {
    // Go discards the error here (`got, _ := tt.id.Value()`); assert on it
    // instead, so a failure cannot masquerade as the expected NULL.
    let got = id
        .value()
        .unwrap_or_else(|e| panic!("{name}: Value() err={e}"));
    assert_eq!(
        got, expected_val,
        "{name}: wanted {expected_val:?}, got {got:?}"
    );
}

/// `TestIDValue/non_nil_id`
#[test]
fn test_id_value_non_nil_id() {
    let i = fixture();
    id_value(
        "non nil id",
        ID { id: i },
        DriverValue::Bytes(i.bytes().to_vec()),
    );
}

/// `TestIDValue/nil_id`
#[test]
fn test_id_value_nil_id() {
    id_value("nil id", ID { id: nil_id() }, DriverValue::Null);
}

// ---------------------------------------------------------------------------
// TestIDScan
// ---------------------------------------------------------------------------

fn id_scan(
    name: &str,
    scan: impl Fn(&mut ID) -> Result<(), xid::Error>,
    expected_id: ID,
    expected_err: bool,
) {
    let mut id = ID::default();
    let err = scan(&mut id);
    assert_eq!(
        err.is_err(),
        expected_err,
        "{name}: error expected: {expected_err}, got {}",
        err.is_err()
    );
    if err.is_ok() {
        assert_eq!(
            id.id, expected_id.id,
            "{name}: wanted {expected_id}, got {id}"
        );
    }
}

/// `TestIDScan/bytes_id`
#[test]
fn test_id_scan_bytes_id() {
    let i = fixture();
    let raw = i.bytes().to_vec();
    id_scan("bytes id", |id| id.scan(&raw), ID { id: i }, false);
}

/// `TestIDScan/nil_id`
#[test]
fn test_id_scan_nil_id() {
    id_scan("nil id", |id| id.scan(()), ID { id: nil_id() }, false);
}

/// `TestIDScan/wrong_bytes`
#[test]
fn test_id_scan_wrong_bytes() {
    id_scan(
        "wrong bytes",
        |id| id.scan(&[0x01u8][..]),
        ID::default(),
        true,
    );
}

/// `TestIDScan/unknown_type`
#[test]
fn test_id_scan_unknown_type() {
    id_scan("unknown type", |id| id.scan(1), ID::default(), true);
}

// ---------------------------------------------------------------------------
// Added by the migration
// ---------------------------------------------------------------------------

#[test]
fn test_id_scan_error_messages() {
    // The two error paths of `b.ID::Scan`, spelled out.
    let mut id = ID::default();
    assert_eq!(
        id.scan(&[0x01u8][..]).unwrap_err().to_string(),
        "xid: invalid ID"
    );
    assert_eq!(
        id.scan(1).unwrap_err().to_string(),
        "xid: scanning unsupported type: int"
    );
}

#[test]
fn test_embedded_methods_are_promoted() {
    // Go's struct embedding promotes the whole `xid.ID` method set.
    let i = fixture();
    let id = ID { id: i };
    assert_eq!(id.string(), "9m4e2mr0ui3e8a215n4g");
    assert_eq!(id.counter(), i.counter());
    assert_eq!(id.pid(), i.pid());
    assert_eq!(id.machine(), i.machine());
    assert!(!id.is_nil());
    assert!(ID::default().is_nil());
}
