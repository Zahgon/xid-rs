//! Port of `TestMachineFromEnv` from `id_test.go`.
//!
//! Isolated in its own test binary: the test mutates `XID_MACHINE_ID`, which
//! is process-wide state that the lazily initialised machine id reads. Go does
//! not need the isolation because `machineID` is computed during package
//! initialisation, before any test runs; giving this test a private process
//! reproduces that guarantee.
//!
//! Go's five `t.Run` cases are separate run units, so each becomes its own
//! `#[test]` carrying the Go subtest's name. They share one process-wide
//! environment variable, so they take a lock — Go gets the same exclusivity
//! for free by running a package's tests sequentially.

use std::panic;
use std::sync::{Mutex, MutexGuard};
use xid::read_machine_id_from_env;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn env_lock() -> MutexGuard<'static, ()> {
    ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Runs one case of the Go table: set `XID_MACHINE_ID`, call
/// `readMachineIDFromEnv`, and check either the decoded value or the panic.
fn machine_from_env(name: &str, value: &str, expect: i64, should_panic: &str) {
    let _guard = env_lock();

    // Keep the ported panics quiet; the assertions below inspect the payload.
    let hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));

    std::env::set_var("XID_MACHINE_ID", value);
    let result = panic::catch_unwind(read_machine_id_from_env);
    std::env::remove_var("XID_MACHINE_ID");

    panic::set_hook(hook);

    match result {
        Err(payload) => {
            let ps = panic_message(&payload);
            assert!(
                !should_panic.is_empty(),
                "{name}: unexpected panic: \"{ps}\""
            );
            assert_eq!(
                should_panic, ps,
                "{name}: expected panic \"{should_panic}\" but got \"{ps}\""
            );
        }
        Ok(b) => {
            assert!(
                should_panic.is_empty(),
                "{name}: expected panic \"{should_panic}\" but got none"
            );
            let b = b.unwrap_or_else(|| {
                panic!("{name}: got no response from readMachineIDFromEnv, expected {expect}")
            });
            let got = (b[0] as i64) << 16 | (b[1] as i64) << 8 | b[2] as i64;
            assert_eq!(
                got, expect,
                "{name}: expected machine id {expect} from env but got {got}"
            );
        }
    }
}

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        String::new()
    }
}

/// `TestMachineFromEnv/basic`
#[test]
fn test_machine_from_env_basic() {
    machine_from_env("basic", "123", 123, "");
}

/// `TestMachineFromEnv/basic_large`
#[test]
fn test_machine_from_env_basic_large() {
    machine_from_env("basic large", "16777214", 16777214, "");
}

/// `TestMachineFromEnv/bad_input_nan`
#[test]
fn test_machine_from_env_bad_input_nan() {
    machine_from_env(
        "bad input nan",
        "abcd",
        0,
        "XID_MACHINE_ID value is set to not a number",
    );
}

/// `TestMachineFromEnv/bad_input_negative`
#[test]
fn test_machine_from_env_bad_input_negative() {
    machine_from_env(
        "bad input negative",
        "-1",
        0,
        "XID_MACHINE_ID out of range for 3 bytes",
    );
}

/// `TestMachineFromEnv/bad_input_large`
#[test]
fn test_machine_from_env_bad_input_large() {
    machine_from_env(
        "bad input large",
        "16777216",
        0,
        "XID_MACHINE_ID out of range for 3 bytes",
    );
}

/// Added by the migration: an unset variable reads as Go's nil slice.
#[test]
fn test_machine_from_env_unset() {
    let _guard = env_lock();
    std::env::remove_var("XID_MACHINE_ID");
    assert_eq!(read_machine_id_from_env(), None);
}
