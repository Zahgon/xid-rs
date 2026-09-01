//! Port of the `Benchmark*` functions in `id_test.go`.
//!
//! `go test -bench` has no Cargo equivalent on stable, so the three parallel
//! benchmarks are reproduced as an example binary:
//!
//! ```text
//! cargo run --release --example bench
//! ```
//!
//! Each one mirrors `b.RunParallel(func(pb *testing.PB) { for pb.Next() {…} })`
//! by spreading the iteration budget over `num_cpus` threads.

use std::hint::black_box;
use std::thread;
use std::time::Instant;
use xid::{from_string, new};

const N: usize = 5_000_000;

fn main() {
    bench("BenchmarkNew", || {
        black_box(new());
    });
    bench("BenchmarkNewString", || {
        black_box(new().string());
    });
    bench("BenchmarkFromString", || {
        black_box(from_string("9m4e2mr0ui3e8a215n4g").ok());
    });
}

fn bench<F>(name: &str, f: F)
where
    F: Fn() + Send + Sync,
{
    let threads = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let per_thread = N / threads;
    let f = &f;

    let start = Instant::now();
    thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(move || {
                for _ in 0..per_thread {
                    f();
                }
            });
        }
    });
    let elapsed = start.elapsed();

    let total = per_thread * threads;
    println!(
        "{name}-{threads}\t{total}\t{:.1} ns/op",
        elapsed.as_nanos() as f64 / total as f64
    );
}
