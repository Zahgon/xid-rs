//! Regenerates the parity fixtures from the **original Go implementation** and
//! checks the committed copies against it.
//!
//! `tests/parity.rs` replays `tests/testdata/*.tsv`, whose expected values were
//! produced by running `github.com/rs/xid` — not by reading its source. This
//! example is the generator for those files, so the claim stays reproducible by
//! a third party:
//!
//! ```sh
//! cargo run --example parity_oracle              # regenerate and diff (default)
//! cargo run --example parity_oracle -- write     # overwrite the fixtures
//! cargo run --example parity_oracle -- encode    # print one corpus
//! cargo run --example parity_oracle -- runtime   # host-specific values
//! cargo run --example parity_oracle -- timecmd   # NewWithTime edge cases
//!
//! # build the oracle against a local checkout instead of the published module
//! cargo run --example parity_oracle -- verify --source /path/to/rs_xid
//! ```
//!
//! The Go program is embedded below rather than shipped as a `.go` file: this
//! is a Rust crate, and a stray Go source tree in it is indistinguishable — to
//! a reader or to a tool — from source that was never translated. Embedding
//! keeps the oracle byte-for-byte reproducible while the repository stays
//! entirely Rust. Requires the Go toolchain (and network access, unless
//! `--source` points at a local checkout).
//!
//! # What each mode emits
//!
//! | mode | rows | contents |
//! |------|------|----------|
//! | `encode` | 496 | The three `id_test.go` fixtures, all-zero, all-`0xFF`, a single-bit sweep over all 96 bits, the eight boundary byte values in each of the 12 positions, and 300 deterministic random ids (`rand.NewSource(42)`). Columns: raw hex, `String()`, `Time().Unix()`, `Machine()`, `Pid()`, `Counter()`, `MarshalJSON()`, `MarshalText()`, `Value()`, `IsNil()`, `xidb.ID.Value()`. |
//! | `decode` | 2,045 | Every one of the 256 byte values at each of the five positions that drive the length, alphabet and canonical-tail checks, a full last-character sweep over 20 random ids, every input length from 0 to 25, and the literals used by the Go tests. Columns: hex of the input bytes, then `ok:<hex>` or `err:<message>:<hex>` — the error text *and* the state of the id after the failure. |
//! | `runtime` | 5 | `Machine()`, `Pid()`, `os.Getpid()`, the counter delta between two `New()` calls and the sorted order of the fixtures — the values that depend on the host and cannot be committed. |
//! | `timecmd` | 14 | `uint32(t.Unix())` over the pre-epoch and post-2106 corpus asserted by `new_with_time_matches_go`. |

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The oracle, verbatim. It links the real `github.com/rs/xid` and prints what
/// that package computes.
const ORACLE_GO: &str = r##"// Golden-vector generator: prints, from the *real* Go implementation, every
// observable output the Rust port must reproduce.
package main

import (
	"encoding/hex"
	"fmt"
	"math/rand"
	"os"
	"strings"
	"time"

	"github.com/rs/xid"
	xidb "github.com/rs/xid/b"
)

func main() {
	which := os.Args[1]
	switch which {
	case "encode":
		encodeGolden()
	case "decode":
		decodeGolden()
	case "runtime":
		runtimeGolden()
	case "timecmd":
		timeGolden()
	}
}

// ids used for the encode sweep: fixtures + edge cases + deterministic random.
func corpus() []xid.ID {
	var out []xid.ID
	add := func(b []byte) {
		id, err := xid.FromBytes(b)
		if err != nil {
			panic(err)
		}
		out = append(out, id)
	}
	add([]byte{0x4d, 0x88, 0xe1, 0x5b, 0x60, 0xf4, 0x86, 0xe4, 0x28, 0x41, 0x2d, 0xc9})
	add(make([]byte, 12))
	add([]byte{0x00, 0x00, 0x00, 0x00, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0x00, 0x00, 0x01})
	add([]byte{0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff})
	// single-bit sweep: every bit position set on its own
	for i := 0; i < 12; i++ {
		for bit := 0; bit < 8; bit++ {
			b := make([]byte, 12)
			b[i] = 1 << uint(bit)
			add(b)
		}
	}
	// boundary byte values in every position (the bit-slice edges of base32)
	for i := 0; i < 12; i++ {
		for _, v := range []byte{0x01, 0x0f, 0x10, 0x1f, 0x7f, 0x80, 0xf0, 0xff} {
			b := make([]byte, 12)
			b[i] = v
			add(b)
		}
	}
	// deterministic random
	r := rand.New(rand.NewSource(42))
	for i := 0; i < 300; i++ {
		b := make([]byte, 12)
		r.Read(b)
		add(b)
	}
	return out
}

func encodeGolden() {
	w := os.Stdout
	for _, id := range corpus() {
		jsonBytes, err := id.MarshalJSON()
		if err != nil {
			panic(err)
		}
		textBytes, err := id.MarshalText()
		if err != nil {
			panic(err)
		}
		val, err := id.Value()
		if err != nil {
			panic(err)
		}
		valStr := "<nil>"
		if val != nil {
			valStr = val.(string)
		}
		bID := xidb.ID{ID: id}
		bval, err := bID.Value()
		if err != nil {
			panic(err)
		}
		bvalStr := "<nil>"
		if bval != nil {
			bvalStr = hex.EncodeToString(bval.([]byte))
		}
		fmt.Fprintf(w, "%s\t%s\t%d\t%s\t%d\t%d\t%s\t%s\t%s\t%t\t%s\n",
			hex.EncodeToString(id.Bytes()),
			id.String(),
			id.Time().Unix(),
			hex.EncodeToString(id.Machine()),
			id.Pid(),
			id.Counter(),
			string(jsonBytes),
			string(textBytes),
			valStr,
			id.IsNil(),
			bvalStr,
		)
	}
}

// decodeGolden feeds a wide range of byte strings through FromString and
// records the exact (id, error) pair Go produces.
func decodeGolden() {
	w := os.Stdout
	seen := map[string]bool{}
	emit := func(b []byte) {
		k := string(b)
		if seen[k] {
			return
		}
		seen[k] = true
		id, err := xid.FromString(string(b))
		res := "ok:" + hex.EncodeToString(id.Bytes())
		if err != nil {
			res = "err:" + err.Error() + ":" + hex.EncodeToString(id.Bytes())
		}
		fmt.Fprintf(w, "%s\t%s\n", hex.EncodeToString(b), res)
	}

	base := []string{
		"9m4e2mr0ui3e8a215n4g",
		"c6e52g2mrqcjl44hf170",
		"c6e52g2mrqcjl44hf179",
		"00000000000000000000",
		"vvvvvvvvvvvvvvvvvvvv",
		"",
		"invalid",
		"9M4E2MR0UI3E8A215N4G",
		"TYjhW2D0huQoQS",
		"TYjhW2D0huQoQS3kdk",
		"9m4e2mr0ui3e8a215n4",   // 19
		"9m4e2mr0ui3e8a215n4gg", // 21
	}
	for _, s := range base {
		emit([]byte(s))
	}

	// every byte value at the positions that drive the length, alphabet and
	// canonical-tail checks (0 and 1 open the id, 18 and 19 close it)
	valid := "9m4e2mr0ui3e8a215n4g"
	for _, i := range []int{0, 1, 9, 18, 19} {
		for c := 0; c < 256; c++ {
			b := []byte(valid)
			b[i] = byte(c)
			emit(b)
		}
	}

	// last-character sweep on several ids: exercises the round-trip check
	r := rand.New(rand.NewSource(7))
	for n := 0; n < 20; n++ {
		raw := make([]byte, 12)
		r.Read(raw)
		id, _ := xid.FromBytes(raw)
		s := []byte(id.String())
		for c := 0; c < 32; c++ {
			b := append([]byte(nil), s...)
			b[19] = "0123456789abcdefghijklmnopqrstuv"[c]
			emit(b)
		}
		// mutate a random interior position too
		for k := 0; k < 5; k++ {
			b := append([]byte(nil), s...)
			b[r.Intn(19)] = "0123456789abcdefghijklmnopqrstuv"[r.Intn(32)]
			emit(b)
		}
	}

	// wrong lengths, all lengths 0..25 of valid alphabet
	for n := 0; n <= 25; n++ {
		b := make([]byte, n)
		for i := range b {
			b[i] = "0123456789abcdefghijklmnopqrstuv"[i%32]
		}
		emit(b)
	}
}

// runtimeGolden prints what this *process* computes for the machine id, the
// pid part and a sorted set — the values the Rust port must agree with when
// run on the same host.
func runtimeGolden() {
	id := xid.New()
	fmt.Printf("machine\t%s\n", hex.EncodeToString(id.Machine()))
	fmt.Printf("pid\t%d\n", id.Pid())
	fmt.Printf("ospid\t%d\n", os.Getpid())

	// counter increments by exactly one per call
	a, b := xid.New(), xid.New()
	fmt.Printf("counterdelta\t%d\n", b.Counter()-a.Counter())

	// sort order of the fixtures
	ids := []xid.ID{
		mustFromBytes([]byte{0x4d, 0x88, 0xe1, 0x5b, 0x60, 0xf4, 0x86, 0xe4, 0x28, 0x41, 0x2d, 0xc9}),
		mustFromBytes(make([]byte, 12)),
		mustFromBytes([]byte{0x00, 0x00, 0x00, 0x00, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0x00, 0x00, 0x01}),
	}
	xid.Sort(ids)
	var parts []string
	for _, i := range ids {
		parts = append(parts, i.String())
	}
	fmt.Printf("sorted\t%s\n", strings.Join(parts, ","))
}

func mustFromString(s string) xid.ID {
	id, err := xid.FromString(s)
	if err != nil {
		panic(err)
	}
	return id
}

func mustFromBytes(b []byte) xid.ID {
	id, err := xid.FromBytes(b)
	if err != nil {
		panic(err)
	}
	return id
}

// timeGolden prints uint32(t.Unix()) for the pre-epoch / post-2106 corpus.
func timeGolden() {
	cases := []struct{ sec, nsec int64 }{
		{0, 0}, {1, 0}, {1300816219, 0}, {1300816219, 999999999},
		{-1, 0}, {0, -500000000}, {-1, -1},
		{1 << 32, 0}, {(1 << 32) + 5, 0}, {(1 << 32) - 1, 0}, {1 << 33, 7},
		{-(1 << 32), 0}, {2147483647, 0}, {4294967295, 999999999},
	}
	for _, c := range cases {
		t := time.Unix(c.sec, c.nsec)
		id := xid.NewWithTime(t)
		fmt.Printf("%d\t%d\t%d\t%s\t%d\n", c.sec, c.nsec, t.Unix(), hex.EncodeToString(id.Bytes()[:4]), id.Time().Unix())
	}
}
"##;

const GO_MOD: &str = "module xidparity\n\ngo 1.16\n\nrequire github.com/rs/xid v1.6.0\n";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args
        .first()
        .filter(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| "verify".to_string());
    let source = args
        .iter()
        .position(|a| a == "--source")
        .and_then(|i| args.get(i + 1))
        .cloned();

    let dir = prepare(source.as_deref());

    match mode.as_str() {
        "verify" => verify(&dir, false),
        "write" => verify(&dir, true),
        "encode" | "decode" | "runtime" | "timecmd" => {
            print!("{}", run(&dir, &mode));
        }
        other => {
            eprintln!(
                "unknown mode {other:?}; expected verify|write|encode|decode|runtime|timecmd"
            );
            std::process::exit(2);
        }
    }
}

/// Writes the oracle into a temporary Go module and returns its directory.
fn prepare(source: Option<&str>) -> PathBuf {
    let dir = std::env::temp_dir().join("xid-parity-oracle");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create oracle dir");

    let mut go_mod = GO_MOD.to_string();
    if let Some(src) = source {
        // Build against a local checkout: no network, and the exact tree under
        // audit rather than whatever the proxy serves.
        let abs = fs::canonicalize(src).expect("--source path");
        go_mod.push_str(&format!(
            "\nreplace github.com/rs/xid => {}\n",
            abs.display()
        ));
    }
    fs::write(dir.join("go.mod"), go_mod).expect("write go.mod");
    fs::write(dir.join("main.go"), ORACLE_GO).expect("write main.go");

    let tidy = Command::new("go")
        .args(["mod", "tidy"])
        .current_dir(&dir)
        .output()
        .expect("run `go mod tidy` — is the Go toolchain installed?");
    assert!(
        tidy.status.success(),
        "go mod tidy failed:\n{}",
        String::from_utf8_lossy(&tidy.stderr)
    );
    dir
}

/// Runs one mode of the oracle and returns its stdout.
fn run(dir: &Path, mode: &str) -> String {
    let out = Command::new("go")
        .args(["run", ".", mode])
        .current_dir(dir)
        .output()
        .expect("run the oracle");
    assert!(
        out.status.success(),
        "oracle {mode} failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("oracle output is UTF-8")
}

/// Regenerates both corpora and compares them with the committed fixtures.
fn verify(dir: &Path, write: bool) {
    let testdata = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata");
    let mut bad = 0;

    for (mode, file) in [
        ("encode", "encode_golden.tsv"),
        ("decode", "decode_golden.tsv"),
    ] {
        let fresh = run(dir, mode);
        let path = testdata.join(file);
        if write {
            fs::write(&path, &fresh).expect("write fixture");
            println!("wrote {} ({} rows)", file, fresh.lines().count());
            continue;
        }
        let committed = fs::read_to_string(&path).expect("read fixture");
        if committed == fresh {
            println!("{file}: {} rows, identical to Go", fresh.lines().count());
        } else {
            bad += 1;
            eprintln!("{file}: DIFFERS from the Go oracle");
            for (n, (a, b)) in committed.lines().zip(fresh.lines()).enumerate() {
                if a != b {
                    eprintln!("  line {}:\n    committed: {a}\n    oracle:    {b}", n + 1);
                    break;
                }
            }
        }
    }

    if bad > 0 {
        std::process::exit(1);
    }
}
