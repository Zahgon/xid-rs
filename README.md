# Globally Unique ID Generator

A 1:1 Rust port of [`github.com/rs/xid`](https://github.com/rs/xid).

Package xid is a globally unique id generator library, ready to safely be used directly in your server code.

Xid uses the Mongo Object ID algorithm to generate globally unique ids with a different serialization ([base32hex](https://datatracker.ietf.org/doc/html/rfc4648#page-10)) to make it shorter when transported as a string:
https://docs.mongodb.org/manual/reference/object-id/

- 4-byte value representing the seconds since the Unix epoch,
- 3-byte machine identifier,
- 2-byte process id, and
- 3-byte counter, starting with a random value.

The binary representation of the id is compatible with Mongo 12 bytes Object IDs.
The string representation is using [base32hex](https://datatracker.ietf.org/doc/html/rfc4648#page-10) (w/o padding) for better space efficiency
when stored in that form (20 bytes). The hex variant of base32 is used to retain the
sortable property of the id.

Xid doesn't use base64 because case sensitivity and the 2 non alphanum chars may be an
issue when transported as a string between various systems. Base36 wasn't retained either
because 1/ it's not standard 2/ the resulting size is not predictable (not bit aligned)
and 3/ it would not remain sortable. To validate a base32 `xid`, expect a 20 chars long,
all lowercase sequence of `a` to `v` letters and `0` to `9` numbers (`[0-9a-v]{20}`).

| Name        | Binary Size | String Size    | Features                                                       |
| ----------- | ----------- | -------------- | -------------------------------------------------------------- |
| [UUID]      | 16 bytes    | 36 chars       | configuration free, not sortable                               |
| [shortuuid] | 16 bytes    | 22 chars       | configuration free, not sortable                               |
| [Snowflake] | 8 bytes     | up to 20 chars | needs machine/DC configuration, needs central server, sortable |
| [MongoID]   | 12 bytes    | 24 chars       | configuration free, sortable                                   |
| xid         | 12 bytes    | 20 chars       | configuration free, sortable                                   |

[UUID]: https://en.wikipedia.org/wiki/Universally_unique_identifier
[shortuuid]: https://github.com/stochastic-technologies/shortuuid
[Snowflake]: https://blog.twitter.com/2010/announcing-snowflake
[MongoID]: https://docs.mongodb.org/manual/reference/object-id/

Features:

- Size: 12 bytes (96 bits), smaller than UUID, larger than snowflake
- Base32 hex encoded by default (20 chars when transported as printable string, still sortable)
- Non configured, you don't need set a unique machine and/or data center id (configurable if needed)
- K-ordered
- Embedded time with 1 second precision
- Unicity guaranteed for 16,777,216 (24 bits) unique ids per second and per host/process
- Lock-free (i.e.: unlike UUIDv1 and v2)

Notes:

- Xid is dependent on the system time, a monotonic counter and so is not cryptographically secure. If unpredictability of IDs is important, you should not use Xids. It is worth noting that most other UUID-like implementations are also not cryptographically secure. You should use libraries that rely on cryptographically secure sources (like /dev/urandom on unix, crypto/rand in golang), if you want a truly random ID generator.
- MachineID can be set by the environmental variable `XID_MACHINE_ID` to allow fine tune control over the generation.

## Usage

```rust
let guid = xid::new();

println!("{guid}");
// Output: 9m4e2mr0ui3e8a215n4g
```

Get `xid` embedded info:

```rust
guid.machine();
guid.pid();
guid.time();
guid.counter();
```

Parse one back:

```rust
let id: xid::ID = "9m4e2mr0ui3e8a215n4g".parse().unwrap();
assert_eq!(id.bytes(), &[0x4d, 0x88, 0xe1, 0x5b, 0x60, 0xf4, 0x86, 0xe4, 0x28, 0x41, 0x2d, 0xc9]);
```

Store the 12 raw bytes in a database instead of the 20-char string with the
`b` module (Go's `xid/b` sub-package):

```rust
let id = xid::b::ID::new(xid::new());
let value = id.value().unwrap(); // DriverValue::Bytes(12 bytes)
```

## Mapping from the Go API

Every exported Go identifier has a counterpart. Where Go relies on interfaces
that Rust does not have, the shape is preserved rather than the mechanism.

| Go                                     | Rust                                                        |
| -------------------------------------- | ----------------------------------------------------------- |
| `xid.ID` (`[12]byte`)                  | `xid::ID` (`[u8; 12]`, `Copy`, `Ord`, `Hash`)                |
| `xid.New()`                            | `xid::new()` / `ID::new()`                                   |
| `xid.NewWithTime(t)`                   | `xid::new_with_time(SystemTime)`                             |
| `xid.FromString(s)`                    | `xid::from_string(&str)` / `s.parse::<ID>()`                 |
| `xid.FromBytes(b)`                     | `xid::from_bytes(&[u8])` / `ID::try_from(&[u8])`             |
| `xid.NilID()`                          | `xid::nil_id()` / `ID::default()`                            |
| `xid.Sort(ids)`                        | `xid::sort(&mut [ID])`                                       |
| `xid.ErrInvalidID`                     | `xid::Error::InvalidId` / `xid::ERR_INVALID_ID`              |
| `id.String()`                          | `id.string()` / `format!("{id}")`                            |
| `id.Encode(dst)`                       | `id.encode(&mut [u8])`                                       |
| `id.MarshalText/JSON()`                | `id.marshal_text()` / `id.marshal_json()`                    |
| `id.UnmarshalText/JSON(b)`             | `id.unmarshal_text(&[u8])` / `id.unmarshal_json(&[u8])`      |
| `id.Time()`                            | `id.time() -> SystemTime` (plus `id.unix_seconds()`)         |
| `id.Machine()/Pid()/Counter()`         | `id.machine()` / `id.pid()` / `id.counter()`                 |
| `id.Value()` (`driver.Valuer`)         | `id.value() -> Result<DriverValue, Error>`                   |
| `id.Scan(v)` (`sql.Scanner`)           | `id.scan(v)`, `v: Into<ScanValue>`                           |
| `id.IsNil()` / `id.IsZero()`           | `id.is_nil()` / `id.is_zero()`                               |
| `id.Bytes()`                           | `id.bytes()`                                                 |
| `id.Compare(other)`                    | `id.compare(&other)` (`-1`/`0`/`1`), plus `Ord`              |
| `sorter` (unexported)                  | `xid::Sorter` with `len`/`less`/`swap`                       |
| `readMachineIDFromEnv` (unexported)    | `xid::read_machine_id_from_env`                              |
| `xidb.ID` (package `b`)                | `xid::b::ID` (`Deref<Target = ID>` emulates Go's embedding)  |

Go's `interface{}` in `Scan` becomes the `ScanValue` enum, and `driver.Value`
becomes `DriverValue`. Each `ScanValue` variant remembers the Go type it stands
for, so the error text is identical to Go's — `id.scan(0)` reports
`xid: scanning unsupported type: int`.

The Go standard-library pieces `id.go` depends on are ported alongside it, so
the crate has **no dependencies**: `gosha256` (`crypto/sha256`), `gocrc32`
(`hash/crc32`), `gorand` (`crypto/rand`), `goos` (`os`/`os/exec`) and `hostid`
(the six `hostid_*.go` build-tagged files, reproduced with `cfg(target_os)`).

## Testing

```sh
cargo test
```

Everything in `id_test.go` and `b/id_test.go` is ported one-for-one, with the
same inputs, assertions and iteration counts (including the 100,000-iteration
padding test and the 1,000/2,000-case `testing/quick` sweeps). On top of that,
`tests/parity.rs` replays 2,541 inputs whose expected outputs were produced by
*running* the original Go package. The generator is embedded in
`examples/parity_oracle.rs`, so the claim is reproducible:

```sh
cargo run --example parity_oracle            # regenerate from Go and diff
cargo run --example parity_oracle -- write   # accept a new corpus
```

It writes the Go oracle into a temporary module, builds it against the real
`github.com/rs/xid`, and compares its output with the committed fixtures. Pass
`-- verify --source /path/to/rs_xid` to build against a local checkout instead
of the published module.

The benchmarks from `id_test.go` are available as an example:

```sh
cargo run --release --example bench
```

## Licenses

All source code is licensed under the [MIT License](https://raw.github.com/rs/xid/master/LICENSE).
