//! Port of Go's `hash/crc32.ChecksumIEEE`.
//!
//! Used by `init()` in `id.go` to fold the contents of `/proc/self/cpuset`
//! into the process id when running inside a container.

/// The IEEE polynomial, in reversed (LSB-first) form — Go's `crc32.IEEE`.
const IEEE_REVERSED: u32 = 0xedb88320;

/// The 256-entry lookup table Go builds with `crc32.MakeTable(crc32.IEEE)`.
static TABLE: [u32; 256] = make_table();

const fn make_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut crc = i as u32;
        let mut j = 0;
        while j < 8 {
            if crc & 1 == 1 {
                crc = (crc >> 1) ^ IEEE_REVERSED;
            } else {
                crc >>= 1;
            }
            j += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

/// Equivalent of Go's `crc32.ChecksumIEEE(data)`.
pub fn checksum_ieee(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xffffffff;
    for &b in data {
        crc = TABLE[((crc as u8) ^ b) as usize] ^ (crc >> 8);
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::hint::black_box;

    /// The table Go builds with `crc32.MakeTable(crc32.IEEE)`.
    #[test]
    fn table_matches_go() {
        // Evaluated at runtime so the const fn itself is exercised.
        let table = black_box(make_table());
        assert_eq!(table, TABLE);
        // Values published in the IEEE 802.3 reflected table.
        assert_eq!(table[0], 0x00000000);
        assert_eq!(table[1], 0x77073096);
        assert_eq!(table[255], 0x2d02ef8d);
    }

    /// Incremental behaviour: the checksum only depends on the bytes.
    #[test]
    fn checksum_is_order_sensitive() {
        assert_ne!(checksum_ieee(b"ab"), checksum_ieee(b"ba"));
        assert_eq!(checksum_ieee(b"abc"), 0x352441c2);
    }
}
