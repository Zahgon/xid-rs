//! End-to-end check of the `XID_MACHINE_ID` override documented in the README
//! ("MachineID can be set by the environmental variable `XID_MACHINE_ID`").
//!
//! Own test binary, and a single test in it, because the machine id is
//! computed once per process — exactly like Go's package-level `machineID`.

use xid::new;

#[test]
fn machine_id_comes_from_env() {
    std::env::set_var("XID_MACHINE_ID", "16777214"); // 0xFFFFFE

    let id = new();
    assert_eq!(
        id.machine(),
        &[0xFF, 0xFF, 0xFE],
        "machine id should be the big-endian encoding of XID_MACHINE_ID"
    );

    // Still stable on a second call: the value is read once per process.
    assert_eq!(new().machine(), &[0xFF, 0xFF, 0xFE]);

    // And the rest of the id is unaffected.
    assert!(!id.is_nil());
    assert_eq!(id.string().len(), 20);
}
