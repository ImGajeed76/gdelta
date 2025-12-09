//! Simple, fast benchmark for gdelta performance and reliability
//!
//! Run: cargo bench --bench simple
//! Compare: cargo bench --bench simple -- --save-baseline main
//!          cargo bench --bench simple -- --baseline main

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use gdelta::{decode, encode};
use rand::{Rng, SeedableRng};
use rand::rngs::StdRng;
use std::fmt::Write;
use std::hint::black_box;

// ============================================================================
// Type Aliases
// ============================================================================

type TestCase = (&'static str, Vec<u8>, fn(&[u8]) -> Vec<u8>);

// ============================================================================
// Test Data Generators
// ============================================================================

fn generate_json(size: usize) -> Vec<u8> {
    let mut data = String::from("[\n");
    let mut rng = StdRng::seed_from_u64(42);

    while data.len() < size {
        writeln!(
            data,
            r#"  {{"id": {}, "name": "user_{}", "email": "user{}@test.com", "active": {}}},"#,
            rng.random_range(1000..99999),
            rng.random_range(0..1000),
            rng.random_range(0..1000),
            rng.random_bool(0.8)
        ).unwrap();
    }

    data.push_str("]\n");
    data.into_bytes()
}

fn generate_logs(size: usize) -> Vec<u8> {
    let mut data = String::new();
    let mut rng = StdRng::seed_from_u64(42);
    let levels = ["INFO", "WARN", "ERROR", "DEBUG"];

    while data.len() < size {
        writeln!(
            data,
            "[{}] {} [thread-{}] Processing request {}",
            1_700_000_000 + rng.random_range(0..1_000_000),
            levels[rng.random_range(0..levels.len())],
            rng.random_range(1..20),
            rng.random_range(1000..99999)
        ).unwrap();
    }

    data.into_bytes()
}

fn generate_csv(size: usize) -> Vec<u8> {
    let mut data = String::from("id,timestamp,value,status\n");
    let mut rng = StdRng::seed_from_u64(42);

    while data.len() < size {
        writeln!(
            data,
            "{},{},{:.2},active",
            rng.random_range(1000..99999),
            1_700_000_000 + rng.random_range(0..1_000_000),
            rng.random_range(0.0..1000.0)
        ).unwrap();
    }

    data.into_bytes()
}

fn generate_binary(size: usize) -> Vec<u8> {
    let mut rng = StdRng::seed_from_u64(42);
    let mut data = Vec::new();

    while data.len() < size {
        // Simulate structured binary with some patterns
        data.extend_from_slice(&[0xFF, 0xFE, 0x00, 0x01]); // header
        for _ in 0..16 {
            data.push(rng.random::<u8>());
        }
    }

    data.truncate(size);
    data
}

fn generate_text(size: usize) -> Vec<u8> {
    let mut data = String::new();
    let mut rng = StdRng::seed_from_u64(42);
    let words = ["the", "quick", "brown", "fox", "jumps", "over", "lazy", "dog"];

    while data.len() < size {
        for _ in 0..10 {
            data.push_str(words[rng.random_range(0..words.len())]);
            data.push(' ');
        }
        data.push('\n');
    }

    data.into_bytes()
}

// ============================================================================
// Change Patterns
// ============================================================================

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]
fn apply_minor_edit(base: &[u8]) -> Vec<u8> {
    let mut new = base.to_vec();
    let mut rng = StdRng::seed_from_u64(123);
    let changes = (base.len() as f32 * 0.01) as usize; // 1% changes

    for _ in 0..changes {
        if !new.is_empty() {
            let len = new.len();
            new[rng.random_range(0..len)] = rng.random();
        }
    }
    new
}

fn apply_append(base: &[u8], append_size: usize) -> Vec<u8> {
    let mut new = base.to_vec();
    let mut rng = StdRng::seed_from_u64(123);
    new.extend((0..append_size).map(|_| rng.random::<u8>()));
    new
}

// ============================================================================
// Benchmarks
// ============================================================================

fn bench_encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("gdelta_encode");

    let test_cases: Vec<TestCase> = vec![
        ("json_16kb", generate_json(16 * 1024), apply_minor_edit),
        ("logs_16kb", generate_logs(16 * 1024), apply_minor_edit),
        ("csv_64kb", generate_csv(64 * 1024), apply_minor_edit),
        ("binary_128kb", generate_binary(128 * 1024), apply_minor_edit),
        ("text_256kb", generate_text(256 * 1024), apply_minor_edit),
    ];

    for (name, base, change_fn) in test_cases {
        let new = change_fn(&base);

        group.throughput(Throughput::Bytes(new.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(name), &(&base, &new), |b, (base, new)| {
            b.iter(|| {
                encode(black_box(new), black_box(base)).unwrap()
            });
        });
    }

    group.finish();
}

fn bench_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("gdelta_decode");

    let test_cases: Vec<TestCase> = vec![
        ("json_16kb", generate_json(16 * 1024), apply_minor_edit),
        ("logs_16kb", generate_logs(16 * 1024), apply_minor_edit),
        ("csv_64kb", generate_csv(64 * 1024), apply_minor_edit),
        ("binary_128kb", generate_binary(128 * 1024), apply_minor_edit),
        ("text_256kb", generate_text(256 * 1024), apply_minor_edit),
    ];

    for (name, base, change_fn) in test_cases {
        let new = change_fn(&base);
        let delta = encode(&new, &base).unwrap();

        group.throughput(Throughput::Bytes(new.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(name), &(&base, &delta), |b, (base, delta)| {
            b.iter(|| {
                decode(black_box(delta), black_box(base)).unwrap()
            });
        });
    }

    group.finish();
}

fn bench_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("gdelta_roundtrip");

    let test_cases: Vec<TestCase> = vec![
        ("json_16kb", generate_json(16 * 1024), apply_minor_edit),
        ("logs_16kb", generate_logs(16 * 1024), apply_minor_edit),
        ("csv_64kb", generate_csv(64 * 1024), apply_minor_edit),
        ("binary_128kb", generate_binary(128 * 1024), apply_minor_edit),
        ("text_256kb", generate_text(256 * 1024), apply_minor_edit),
    ];

    for (name, base, change_fn) in test_cases {
        let new = change_fn(&base);

        group.throughput(Throughput::Bytes(new.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(name), &(&base, &new), |b, (base, new)| {
            b.iter(|| {
                let delta = encode(black_box(new), black_box(base)).unwrap();
                let reconstructed = decode(black_box(&delta), black_box(base)).unwrap();
                assert_eq!(reconstructed.len(), new.len(), "Size mismatch in reconstruction");
            });
        });
    }

    group.finish();
}

#[allow(clippy::cast_precision_loss)]
fn bench_compression_ratio(c: &mut Criterion) {
    println!("\n=== Compression Ratio Tests ===\n");

    let test_cases = vec![
        ("json_16kb_minor", generate_json(16 * 1024), apply_minor_edit as fn(&[u8]) -> Vec<u8>),
        ("json_16kb_append", generate_json(16 * 1024), (|b: &[u8]| apply_append(b, 1024)) as fn(&[u8]) -> Vec<u8>),
        ("logs_64kb_minor", generate_logs(64 * 1024), apply_minor_edit as fn(&[u8]) -> Vec<u8>),
        ("csv_128kb_minor", generate_csv(128 * 1024), apply_minor_edit as fn(&[u8]) -> Vec<u8>),
        ("binary_256kb_minor", generate_binary(256 * 1024), apply_minor_edit as fn(&[u8]) -> Vec<u8>),
    ];

    let mut all_passed = true;

    for (name, base, change_fn) in test_cases {
        let new = change_fn(&base);

        match encode(&new, &base) {
            Ok(delta) => {
                match decode(&delta, &base) {
                    Ok(reconstructed) => {
                        let passed = reconstructed == new;
                        let ratio = delta.len() as f64 / new.len() as f64;
                        let savings = (1.0 - ratio) * 100.0;

                        let status = if passed { "✓" } else { "✗" };
                        all_passed = all_passed && passed;

                        println!(
                            "{status} {name:30} | Base: {:>7} | New: {:>7} | Delta: {:>7} | Ratio: {:>5.1}% | Saved: {:>5.1}%",
                            format_size(base.len()),
                            format_size(new.len()),
                            format_size(delta.len()),
                            ratio * 100.0,
                            savings
                        );

                        if !passed {
                            println!("  ERROR: Reconstruction mismatch! Expected {} bytes, got {} bytes",
                                     new.len(), reconstructed.len());
                        }
                    }
                    Err(e) => {
                        println!("✗ {name} | DECODE FAILED: {e}");
                        all_passed = false;
                    }
                }
            }
            Err(e) => {
                println!("✗ {name} | ENCODE FAILED: {e}");
                all_passed = false;
            }
        }
    }

    println!();
    if all_passed {
        println!("✅ All correctness checks passed!\n");
    } else {
        println!("❌ Some tests failed - check output above\n");
    }

    // Run a minimal benchmark just to keep criterion happy
    c.bench_function("compression_check", |b| b.iter(|| {}));
}

fn format_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{} KB", bytes / 1024)
    } else {
        format!("{} MB", bytes / (1024 * 1024))
    }
}

criterion_group!(
    benches,
    bench_compression_ratio,
    bench_encode,
    bench_decode,
    bench_roundtrip
);
criterion_main!(benches);