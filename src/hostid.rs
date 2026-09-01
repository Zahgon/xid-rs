//! Port of `hostid_darwin.go`, `hostid_linux.go`, `hostid_freebsd.go`,
//! `hostid_openbsd.go`, `hostid_windows.go` and `hostid_fallback.go`.
//!
//! Go selects one of those files with build tags; Rust selects one of the
//! `cfg(target_os = …)` blocks below. Every implementation keeps the original
//! `(string, error)` contract, including the quirks (`hostid_linux.go` returns
//! the *second* read's error even when the first file is the one that was
//! missing, and returns the file contents verbatim — trailing newline and all).

use std::io;

/// Reads a platform-specific machine id.
///
/// Direct counterpart of Go's `readPlatformMachineID`.
#[cfg(target_os = "macos")]
pub fn read_platform_machine_id() -> io::Result<String> {
    use crate::goos::look_path;
    use std::process::Command;

    let ioreg = look_path("ioreg")?;

    // Go: cmd.CombinedOutput() — stdout and stderr into a single buffer.
    let output = Command::new(ioreg)
        .args(["-rd1", "-c", "IOPlatformExpertDevice"])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("exit status {}", exit_code(&output.status)),
        ));
    }
    let mut out = output.stdout;
    out.extend_from_slice(&output.stderr);

    parse_ioreg_output(&String::from_utf8_lossy(&out))
}

/// The scanning half of `readPlatformMachineID`, split out so the parse can be
/// tested against recorded `ioreg` output instead of only whatever this host
/// happens to report.
#[cfg(target_os = "macos")]
fn parse_ioreg_output(out: &str) -> io::Result<String> {
    for line in out.split('\n') {
        if line.contains("IOPlatformUUID") {
            // Go: strings.SplitAfter(line, `" = "`) — exactly two parts means
            // the separator occurs exactly once.
            const SEP: &str = "\" = \"";
            if line.matches(SEP).count() == 1 {
                let tail = &line[line.find(SEP).unwrap() + SEP.len()..];
                let uuid = tail.trim_end_matches('"');
                return Ok(uuid.to_lowercase());
            }
        }
    }

    Err(io::Error::new(io::ErrorKind::Other, "cannot find host id"))
}

#[cfg(target_os = "macos")]
fn exit_code(status: &std::process::ExitStatus) -> i32 {
    status.code().unwrap_or(-1)
}

#[cfg(all(test, target_os = "macos"))]
mod darwin_tests {
    use super::*;

    /// Real `ioreg -rd1 -c IOPlatformExpertDevice` output, trimmed.
    const IOREG: &str = r#"+-o Root  <class IORegistryEntry, id 0x100000100, retain 15>
  +-o MacBookPro18,3  <class IOPlatformExpertDevice, id 0x100000278, registered>
      {
        "IOPlatformUUID" = "1B2C3D4E-5F60-4A7B-8C9D-0E1F2A3B4C5D"
        "IOPlatformSerialNumber" = "C02XY0ABCD"
        "board-id" = <"Mac-1234567890ABCDEF">
      }
"#;

    #[test]
    fn parses_the_platform_uuid_and_lowercases_it() {
        // Go lowercases the UUID before hashing it.
        assert_eq!(
            parse_ioreg_output(IOREG).unwrap(),
            "1b2c3d4e-5f60-4a7b-8c9d-0e1f2a3b4c5d"
        );
    }

    #[test]
    fn requires_exactly_one_separator() {
        // Go: `len(parts) == 2` after SplitAfter — a second `" = "` on the
        // line disqualifies it, and the scan continues.
        let two_seps = "\"IOPlatformUUID\" = \"a\" = \"b\"\n";
        assert!(parse_ioreg_output(two_seps).is_err());

        // No separator at all: also skipped.
        assert!(parse_ioreg_output("\"IOPlatformUUID\" is missing\n").is_err());
    }

    #[test]
    fn reports_cannot_find_host_id() {
        let err = parse_ioreg_output("+-o Root\n  nothing here\n").unwrap_err();
        assert_eq!(err.to_string(), "cannot find host id");
        assert!(parse_ioreg_output("").is_err());
    }

    #[test]
    fn takes_the_first_matching_line() {
        let two = format!(
            "{IOREG}\n        \"IOPlatformUUID\" = \"FFFFFFFF-0000-0000-0000-000000000000\"\n"
        );
        assert_eq!(
            parse_ioreg_output(&two).unwrap(),
            "1b2c3d4e-5f60-4a7b-8c9d-0e1f2a3b4c5d"
        );
    }

    #[test]
    fn exit_code_reports_the_process_status() {
        // Exercises the non-zero-exit branch's helper the way a failing
        // `ioreg` would.
        let ok = std::process::Command::new("/usr/bin/true")
            .output()
            .unwrap();
        assert_eq!(exit_code(&ok.status), 0);
        let bad = std::process::Command::new("/usr/bin/false")
            .output()
            .unwrap();
        assert_eq!(exit_code(&bad.status), 1);
    }

    #[test]
    fn live_read_agrees_with_the_parser() {
        // The real call must still work on this host, and must equal what the
        // parser extracts from the same command's output.
        let live = read_platform_machine_id().expect("ioreg");
        let out = std::process::Command::new("ioreg")
            .args(["-rd1", "-c", "IOPlatformExpertDevice"])
            .output()
            .expect("ioreg");
        let mut buf = out.stdout;
        buf.extend_from_slice(&out.stderr);
        assert_eq!(
            live,
            parse_ioreg_output(&String::from_utf8_lossy(&buf)).unwrap()
        );
        assert!(!live.is_empty());
        assert_eq!(live, live.to_lowercase());
    }
}

/// Reads a platform-specific machine id.
///
/// Direct counterpart of Go's `readPlatformMachineID`.
#[cfg(target_os = "linux")]
pub fn read_platform_machine_id() -> io::Result<String> {
    use crate::goos::read_file;

    // Transcription of:
    //
    //   b, err := os.ReadFile("/etc/machine-id")
    //   if err != nil || len(b) == 0 {
    //       b, err = os.ReadFile("/sys/class/dmi/id/product_uuid")
    //   }
    //   return string(b), err
    let mut b: Vec<u8> = Vec::new();
    let mut err: Option<io::Error> = None;
    match read_file("/etc/machine-id") {
        Ok(v) => b = v,
        Err(e) => err = Some(e),
    }
    if err.is_some() || b.is_empty() {
        b = Vec::new();
        err = None;
        match read_file("/sys/class/dmi/id/product_uuid") {
            Ok(v) => b = v,
            Err(e) => err = Some(e),
        }
    }
    match err {
        // Go returns `string(b), err` — a non-nil error with an empty string.
        Some(e) => Err(e),
        None => Ok(String::from_utf8_lossy(&b).into_owned()),
    }
}

/// Reads a platform-specific machine id.
///
/// Direct counterpart of Go's `readPlatformMachineID`.
#[cfg(target_os = "freebsd")]
pub fn read_platform_machine_id() -> io::Result<String> {
    sysctl("kern.hostuuid")
}

/// Reads a platform-specific machine id.
///
/// Direct counterpart of Go's `readPlatformMachineID`.
#[cfg(target_os = "openbsd")]
pub fn read_platform_machine_id() -> io::Result<String> {
    sysctl("hw.uuid")
}

/// Port of Go's `syscall.Sysctl` (the by-name string flavour).
#[cfg(any(target_os = "freebsd", target_os = "openbsd"))]
fn sysctl(name: &str) -> io::Result<String> {
    use std::ffi::CString;
    use std::os::raw::{c_char, c_int, c_void};

    extern "C" {
        fn sysctlbyname(
            name: *const c_char,
            oldp: *mut c_void,
            oldlenp: *mut usize,
            newp: *mut c_void,
            newlen: usize,
        ) -> c_int;
    }

    let cname = CString::new(name).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;

    // First call: ask for the size.
    let mut len: usize = 0;
    let rc = unsafe {
        sysctlbyname(
            cname.as_ptr(),
            std::ptr::null_mut(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    if len == 0 {
        return Ok(String::new());
    }

    // Second call: fetch the value.
    let mut buf = vec![0u8; len];
    let rc = unsafe {
        sysctlbyname(
            cname.as_ptr(),
            buf.as_mut_ptr() as *mut c_void,
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    buf.truncate(len);
    // Go drops the trailing NUL byte.
    if buf.last() == Some(&0) {
        buf.pop();
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Reads a platform-specific machine id.
///
/// Direct counterpart of Go's `readPlatformMachineID`, reading
/// `HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid`.
#[cfg(windows)]
pub fn read_platform_machine_id() -> io::Result<String> {
    type Hkey = isize;
    const HKEY_LOCAL_MACHINE: Hkey = -2147483646; // 0x80000002
    const KEY_READ: u32 = 0x20019;
    const KEY_WOW64_64KEY: u32 = 0x0100;

    #[link(name = "advapi32")]
    extern "system" {
        fn RegOpenKeyExW(
            key: Hkey,
            sub_key: *const u16,
            options: u32,
            desired: u32,
            result: *mut Hkey,
        ) -> i32;
        fn RegQueryValueExW(
            key: Hkey,
            value_name: *const u16,
            reserved: *mut u32,
            value_type: *mut u32,
            data: *mut u8,
            data_len: *mut u32,
        ) -> i32;
        fn RegCloseKey(key: Hkey) -> i32;
    }

    fn utf16z(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    // len(`{`) + len(`abcdefgh-1234-456789012-123345456671` * 2) + len(`}`)
    const SYSCALL_REG_BUF_LEN: usize = 74;
    const UUID_LEN: usize = 36;

    let sub_key = utf16z(r"SOFTWARE\Microsoft\Cryptography");
    let mut h: Hkey = 0;
    let rc = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            sub_key.as_ptr(),
            0,
            KEY_READ | KEY_WOW64_64KEY,
            &mut h,
        )
    };
    if rc != 0 {
        return Err(io::Error::from_raw_os_error(rc));
    }

    struct KeyGuard(Hkey);
    impl Drop for KeyGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = RegCloseKey(self.0);
            }
        }
    }
    let _guard = KeyGuard(h);

    let mut reg_buf = [0u16; SYSCALL_REG_BUF_LEN];
    let mut buf_len: u32 = SYSCALL_REG_BUF_LEN as u32;
    let mut val_type: u32 = 0;
    let value_name = utf16z("MachineGuid");

    let rc = unsafe {
        RegQueryValueExW(
            h,
            value_name.as_ptr(),
            std::ptr::null_mut(),
            &mut val_type,
            reg_buf.as_mut_ptr() as *mut u8,
            &mut buf_len,
        )
    };
    if rc != 0 {
        return Err(io::Error::new(io::ErrorKind::Other, "error parsing "));
    }

    // syscall.UTF16ToString: stop at the first NUL.
    let end = reg_buf
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(reg_buf.len());
    let host_id = String::from_utf16_lossy(&reg_buf[..end]);
    if host_id.chars().count() != UUID_LEN {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("HostID incorrect: {host_id:?}\n"),
        ));
    }
    Ok(host_id)
}

/// Reads a platform-specific machine id.
///
/// Counterpart of `hostid_fallback.go` — every OS without a dedicated reader.
#[cfg(not(any(
    target_os = "macos",
    target_os = "linux",
    target_os = "freebsd",
    target_os = "openbsd",
    windows
)))]
pub fn read_platform_machine_id() -> io::Result<String> {
    Err(io::Error::new(io::ErrorKind::Other, "not implemented"))
}
