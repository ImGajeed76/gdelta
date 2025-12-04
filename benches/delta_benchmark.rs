//! Benchmarks for gdelta encode and decode operations.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use gdelta::{decode, encode};
use std::hint::black_box;

fn create_test_data(size: usize, change_rate: usize) -> (Vec<u8>, Vec<u8>) {
    let mut base = vec![0u8; size];
    let mut new = vec![0u8; size];

    // Fill with pattern
    for i in 0..size {
        base[i] = (i % 256) as u8;
        new[i] = (i % 256) as u8;
    }

    // Make modifications at specified intervals
    for i in (0..size).step_by(change_rate) {
        if i < size {
            new[i] = new[i].wrapping_add(1);
        }
    }

    (base, new)
}

fn benchmark_encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("encode");

    for size in [1024, 10 * 1024, 100 * 1024].iter() {
        let (base, new) = create_test_data(*size, 100);

        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| encode(black_box(&new), black_box(&base)))
        });
    }

    group.finish();
}

fn benchmark_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("decode");

    for size in [1024, 10 * 1024, 100 * 1024].iter() {
        let (base, new) = create_test_data(*size, 100);
        let delta = encode(&new, &base).unwrap();

        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| decode(black_box(&delta), black_box(&base)))
        });
    }

    group.finish();
}

fn benchmark_similarity(c: &mut Criterion) {
    let mut group = c.benchmark_group("similarity");
    let size = 50 * 1024;

    for change_rate in [50, 100, 500, 1000].iter() {
        let (base, new) = create_test_data(size, *change_rate);

        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(
            BenchmarkId::new("encode", change_rate),
            change_rate,
            |b, _| b.iter(|| encode(black_box(&new), black_box(&base))),
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    benchmark_encode,
    benchmark_decode,
    benchmark_similarity
);
criterion_main!(benches);
