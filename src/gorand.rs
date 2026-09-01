//! Port of the pieces of Go's `crypto/rand` that `xid` relies on.
//!
//! `id.go` uses exactly one entry point:
//!
//! ```go
//! if _, err := rand.Reader.Read(b); err != nil { ... }
//! ```
//!
//! Go's `crypto/rand.Reader` is documented to always fill the whole buffer or
//! return an error, so the Rust counterpart is a "fill or fail" function. The
//! error is surfaced (not swallowed) because both call sites in `xid` panic
//! with the error text embedded in the message.

use std::io;

/// Fills `b` with cryptographically secure random bytes.
///
/// Equivalent of Go's `rand.Reader.Read(b)` for the "read everything" contract
/// that `crypto/rand` guarantees.
pub fn read(b: &mut [u8]) -> io::Result<()> {
    if b.is_empty() {
        return Ok(());
    }
    fill(b)
}

#[cfg(unix)]
fn fill(b: &mut [u8]) -> io::Result<()> {
    use std::fs::File;
    use std::io::Read;
    use std::sync::{Mutex, OnceLock};

    // Go keeps a single lazily-opened handle in `crypto/rand`; do the same so
    // repeated calls do not pay for an open() each time.
    static SOURCE: OnceLock<io::Result<Mutex<File>>> = OnceLock::new();

    let source = SOURCE.get_or_init(|| File::open("/dev/urandom").map(Mutex::new));
    match source {
        Ok(file) => {
            let mut guard = file.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            guard.read_exact(b)
        }
        Err(e) => Err(io::Error::new(e.kind(), e.to_string())),
    }
}

#[cfg(windows)]
fn fill(b: &mut [u8]) -> io::Result<()> {
    // `RtlGenRandom`, exported from advapi32 under the name `SystemFunction036`.
    // This is what Go's `crypto/rand` used on Windows before it moved to
    // `ProcessPrng`; both are backed by the same CSPRNG.
    #[link(name = "advapi32")]
    extern "system" {
        #[link_name = "SystemFunction036"]
        fn rtl_gen_random(random_buffer: *mut u8, random_buffer_length: u32) -> u8;
    }

    // The API takes a u32 length; chunk to stay within it.
    for chunk in b.chunks_mut(u32::MAX as usize) {
        let ok = unsafe { rtl_gen_random(chunk.as_mut_ptr(), chunk.len() as u32) };
        if ok == 0 {
            return Err(io::Error::new(io::ErrorKind::Other, "RtlGenRandom failed"));
        }
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn fill(_b: &mut [u8]) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "crypto/rand: no secure random source on this platform",
    ))
}
