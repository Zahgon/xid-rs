//! Port of the handful of `os`/`os/exec` helpers `xid` uses.
//!
//! Go call sites mirrored here:
//!
//! | Go                     | Rust                    |
//! |------------------------|-------------------------|
//! | `os.ReadFile(name)`    | [`read_file`]           |
//! | `os.Hostname()`        | [`hostname`]            |
//! | `os.Getpid()`          | [`getpid`]              |
//! | `os.Getenv(key)`       | [`getenv`]              |
//! | `exec.LookPath(file)`  | [`look_path`]           |
//!
//! The signatures keep Go's `(value, error)` shape as `io::Result<T>` so the
//! branching in `readMachineID` can be transcribed one line at a time.

use std::io;
use std::path::PathBuf;

/// Equivalent of Go's `os.ReadFile`.
pub fn read_file(name: &str) -> io::Result<Vec<u8>> {
    std::fs::read(name)
}

/// Equivalent of Go's `os.Getenv`: a missing (or non-UTF-8) variable reads as
/// the empty string, exactly like Go.
pub fn getenv(key: &str) -> String {
    std::env::var(key).unwrap_or_default()
}

/// Equivalent of Go's `os.Getpid`.
///
/// Go returns `int`; the wider type matters because `id.go` xors the pid with
/// a `uint32` CRC and relies on the result staying a 64-bit signed value.
pub fn getpid() -> i64 {
    std::process::id() as i64
}

/// Equivalent of Go's `os.Hostname`.
#[cfg(unix)]
pub fn hostname() -> io::Result<String> {
    use std::os::raw::{c_char, c_int};

    extern "C" {
        fn gethostname(name: *mut c_char, len: usize) -> c_int;
    }

    // Go sizes this buffer from sysconf(_SC_HOST_NAME_MAX); 512 is comfortably
    // above every platform's limit (Linux 64, macOS 255).
    let mut buf = [0u8; 512];
    let rc = unsafe { gethostname(buf.as_mut_ptr() as *mut c_char, buf.len()) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    // The name is NUL-terminated; truncation without a NUL is possible on some
    // systems, so bound the search by the buffer length.
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Ok(String::from_utf8_lossy(&buf[..end]).into_owned())
}

/// Equivalent of Go's `os.Hostname`.
#[cfg(windows)]
pub fn hostname() -> io::Result<String> {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetComputerNameExW(name_type: u32, buffer: *mut u16, size: *mut u32) -> i32;
    }
    // ComputerNameDnsHostname — what Go's `os.Hostname` asks for on Windows.
    const COMPUTER_NAME_DNS_HOSTNAME: u32 = 1;

    let mut size: u32 = 256;
    let mut buf = vec![0u16; size as usize];
    let ok = unsafe { GetComputerNameExW(COMPUTER_NAME_DNS_HOSTNAME, buf.as_mut_ptr(), &mut size) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    let end = core::cmp::min(size as usize, buf.len());
    Ok(String::from_utf16_lossy(&buf[..end]))
}

#[cfg(not(any(unix, windows)))]
pub fn hostname() -> io::Result<String> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "os: Hostname not supported on this platform",
    ))
}

/// Equivalent of Go's `exec.LookPath`: searches `PATH` for an executable file.
///
/// Only the darwin machine-id reader needs this, but it is kept faithful (an
/// error when the binary is absent) because that error is what makes
/// `readMachineID` fall back to the hostname.
#[allow(dead_code)]
pub fn look_path(file: &str) -> io::Result<PathBuf> {
    // Go: "If file contains a slash, it is tried directly."
    if file.contains('/') {
        return if is_executable(std::path::Path::new(file)) {
            Ok(PathBuf::from(file))
        } else {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("exec: \"{file}\": permission denied"),
            ))
        };
    }

    let path = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path) {
        // Go treats an empty PATH element as the current directory.
        let dir = if dir.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            dir
        };
        let candidate = dir.join(file);
        if is_executable(&candidate) {
            return Ok(candidate);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!("exec: \"{file}\": executable file not found in $PATH"),
    ))
}

#[allow(dead_code)]
fn is_executable(path: &std::path::Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(path) {
            Ok(md) => md.is_file() && md.permissions().mode() & 0o111 != 0,
            Err(_) => false,
        }
    }
    #[cfg(not(unix))]
    {
        std::fs::metadata(path)
            .map(|md| md.is_file())
            .unwrap_or(false)
    }
}
