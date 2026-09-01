//! Tests for the ported Go standard-library pieces, and for the runtime
//! derivation of the machine id / pid.
//!
//! `id.go` reaches into four stdlib packages (`crypto/sha256`, `hash/crc32`,
//! `crypto/rand`, `os`). The Go test-suite covers them only indirectly, but a
//! silent divergence in any of them would change every generated id on the
//! affected host, so each is pinned here against published vectors.

mod common;

use common::gen_lock;
use xid::{gocrc32::checksum_ieee, goos, gorand, gosha256, hostid, new};

#[test]
fn sha256_matches_go() {
    // FIPS 180-4 / RFC 6234 vectors — identical to what Go's crypto/sha256
    // produces for the same inputs.
    let cases: [(&str, &str); 4] = [
        (
            "",
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        ),
        (
            "abc",
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        ),
        (
            "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
        ),
        (
            "abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu",
            "cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1",
        ),
    ];

    for (input, want) in cases.iter() {
        let got: String = gosha256::sum256(input.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(&got, want, "sha256({input:?})");
    }

    // One million 'a' characters — exercises the streaming/blocking path.
    let mut d = gosha256::Digest::new();
    for _ in 0..1000 {
        d.write(&[b'a'; 1000]);
    }
    let got: String = d.sum().iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(
        got, "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0",
        "sha256(a*1e6)"
    );

    // Chunked writes must equal a single write (readMachineID writes once, but
    // the streaming API is part of the port).
    let data = b"the quick brown fox jumps over the lazy dog, repeatedly and at length";
    let one = gosha256::sum256(data);
    let mut split = gosha256::Digest::new();
    split.write(&data[..7]);
    split.write(&data[7..30]);
    split.write(&data[30..]);
    assert_eq!(split.sum(), one, "chunked write");

    // Sum() must not consume the digest (Go's Sum does not either).
    assert_eq!(split.sum(), one, "Sum() is idempotent");
}

#[test]
fn crc32_matches_go() {
    // crc32.ChecksumIEEE vectors.
    assert_eq!(checksum_ieee(b""), 0x00000000);
    assert_eq!(checksum_ieee(b"a"), 0xe8b7be43);
    assert_eq!(checksum_ieee(b"abc"), 0x352441c2);
    assert_eq!(checksum_ieee(b"123456789"), 0xcbf43926);
    assert_eq!(
        checksum_ieee(b"The quick brown fox jumps over the lazy dog"),
        0x414fa339
    );
    // The shape `init()` actually hashes: a cgroup cpuset path.
    assert_eq!(checksum_ieee(b"/docker/0123456789abcdef\n"), 0xdab84d51);
    assert_eq!(checksum_ieee(b"/"), 0x79d3d2d4);
}

#[test]
fn crypto_rand_fills_the_buffer() {
    // `randInt` reads 3 bytes and panics on error; make sure the source works
    // and is not returning a constant.
    let mut a = [0u8; 32];
    let mut b = [0u8; 32];
    gorand::read(&mut a).expect("crypto/rand");
    gorand::read(&mut b).expect("crypto/rand");
    assert_ne!(a, b, "two reads should differ");
    assert!(a.iter().any(|&x| x != 0), "read should not be all zeros");
    // A zero-length read is a no-op, like Go's.
    gorand::read(&mut []).expect("empty read");
}

#[test]
fn machine_id_and_pid_derivation() {
    let _guard = gen_lock();
    // Guard: this process must not have inherited the env override.
    assert!(
        std::env::var("XID_MACHINE_ID").is_err(),
        "test assumes XID_MACHINE_ID is unset"
    );

    let id = new();

    // The machine id is the first 3 bytes of sha256(platform machine id), or
    // of sha256(hostname) when the platform has no machine id.
    let hid = match hostid::read_platform_machine_id() {
        Ok(h) if !h.is_empty() => h,
        _ => goos::hostname().expect("hostname"),
    };
    let want = &gosha256::sum256(hid.as_bytes())[..3];
    assert_eq!(id.machine(), want, "machine id derivation");

    // Stable across calls.
    assert_eq!(new().machine(), id.machine());

    // The pid part is the low 16 bits of the (possibly cpuset-xored) pid.
    // Off Linux there is no /proc/self/cpuset, so it is the plain pid.
    #[cfg(not(target_os = "linux"))]
    {
        assert!(
            !std::path::Path::new("/proc/self/cpuset").exists(),
            "unexpected cpuset file"
        );
        assert_eq!(id.pid() as u32, std::process::id() & 0xFFFF, "pid part");
    }
    #[cfg(target_os = "linux")]
    {
        let mut want_pid = std::process::id() as i64;
        if let Ok(b) = std::fs::read("/proc/self/cpuset") {
            if b.len() > 1 {
                want_pid ^= checksum_ieee(&b) as i64;
            }
        }
        assert_eq!(id.pid(), want_pid as u16, "pid part");
    }
}

#[test]
fn counter_increments_by_one_and_is_seeded_randomly() {
    let _guard = gen_lock();
    let a = new();
    let b = new();
    assert_eq!(b.counter().wrapping_sub(a.counter()), 1);

    // The counter starts from a random value, so it is essentially never 1 on
    // the first call — assert only the invariant that it is a 24-bit value.
    assert!((0..=0xFF_FFFF).contains(&a.counter()));
}

/// `objectIDCounter = randInt()` — the counter is seeded from `crypto/rand`
/// once per process.
///
/// This is invisible from inside a single process (the seed is consumed before
/// any test can observe it), so the check re-executes this test binary and
/// compares the first counter each child produces. Without it, a port that
/// seeded the counter with a constant would pass every other test in the
/// suite, Go's included.
#[test]
fn counter_seed_differs_between_processes() {
    // Child mode: print the first counter of a fresh process and exit.
    if std::env::var_os("XID_PRINT_FIRST_COUNTER").is_some() {
        println!("FIRST_COUNTER={}", new().counter());
        return;
    }

    let exe = std::env::current_exe().expect("current_exe");
    let first_counter = || -> i32 {
        let out = std::process::Command::new(&exe)
            .args([
                "counter_seed_differs_between_processes",
                "--exact",
                "--nocapture",
            ])
            .env("XID_PRINT_FIRST_COUNTER", "1")
            .output()
            .expect("re-exec test binary");
        assert!(out.status.success(), "child run failed");
        let stdout = String::from_utf8_lossy(&out.stdout);
        stdout
            .lines()
            .find_map(|l| l.strip_prefix("FIRST_COUNTER="))
            .expect("child printed no counter")
            .trim()
            .parse()
            .expect("counter is an integer")
    };

    let seeds = [first_counter(), first_counter(), first_counter()];
    for s in seeds {
        assert!((0..=0xFF_FFFF).contains(&s), "counter is a 24-bit value");
    }
    // Three independent 24-bit draws collide with probability ~2^-48.
    assert!(
        seeds[0] != seeds[1] || seeds[1] != seeds[2],
        "counter seed is not random across processes: {seeds:?}"
    );
}

#[test]
fn concurrent_generation_is_unique() {
    let _guard = gen_lock();
    // Go's counter is atomic; the port must be too. 8 threads × 2,000 ids.
    use std::collections::HashSet;
    let ids: Vec<xid::ID> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..8)
            .map(|_| s.spawn(|| (0..2000).map(|_| new()).collect::<Vec<_>>()))
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap())
            .collect()
    });
    let unique: HashSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len(), "generated ids must be unique");
}
