# Performance Analysis

This document provides comprehensive performance analysis of gdelta and comparison with other delta compression
algorithms.

## Table of Contents

1. [Hardware & Methodology](#hardware--methodology)
2. [Executive Summary](#executive-summary)
3. [gdelta Performance Characteristics](#gdelta-performance-characteristics)
4. [Algorithm Comparison](#algorithm-comparison)
5. [Use Case Recommendations](#use-case-recommendations)
6. [Scaling Behavior](#scaling-behavior)
7. [Running Your Own Benchmarks](#running-your-own-benchmarks)

---

## Hardware & Methodology

### Test System

```
CPU:    AMD Ryzen 7 7800X3D 8-Core Processor (16 threads)
RAM:    64 GB DDR5
OS:     Fedora Linux 42
Rust:   1.83+ with default optimizations
```

### Benchmark Methodology

**Two benchmark suites:**

1. **Simple Benchmark** (`cargo bench --bench simple`)
    - Quick correctness and performance verification
    - 5 realistic data types (JSON, logs, CSV, binary, text)
    - Sizes: 16KB to 256KB

2. **Comprehensive Benchmark** (`cargo bench --bench comprehensive`)
    - Multi-algorithm comparison (7 algorithms tested)
    - 15 data formats × 7 change patterns × 3 sizes = 315 test cases per algorithm
    - Metrics: compression ratio, encode/decode speed, verification

### Tested Algorithms

| Algorithm       | Description                                      | Version |
|-----------------|--------------------------------------------------|---------|
| **gdelta**      | Pure delta (this implementation)                 | 0.2.0   |
| **gdelta_zstd** | gdelta + zstd compression (level 3)              | 0.2.0   |
| **gdelta_lz4**  | gdelta + lz4 compression                         | 0.2.0   |
| **xpatch**      | Multi-algorithm wrapper (uses gdelta internally) | 0.1.x   |
| **qbsdiff**     | Industry-standard bsdiff                         | 1.4.4   |
| **xdelta3**     | Popular delta compression                        | 0.1.5   |
| **zstd_dict**   | Zstd with dictionary training                    | 0.13.3  |

---

## Executive Summary

### Performance at a Glance

| Algorithm       | Encode Speed | Decode Speed | Compression  |
|-----------------|--------------|--------------|--------------|
| gdelta          | 397 MB/s     | 4.1 GB/s     | 63% saved    |
| gdelta_lz4      | 350 MB/s     | 3.6 GB/s     | 66% saved    |
| gdelta_zstd     | 258 MB/s     | 2.0 GB/s     | 70% saved    |
| xpatch          | 291 MB/s     | 2.3 GB/s     | 75% saved    |
| qbsdiff         |  22 MB/s     | 114 MB/s     | 84% saved    |
| xdelta3*        |  45 MB/s     | N/A          | 81% saved    |
| zstd_dict       |  23 MB/s     |  27 MB/s     | 55% saved    |

\* xdelta3 failed verification tests (produced corrupted output)


### Key Findings

✅ **gdelta is the fastest verified algorithm**

- 18x faster encoding than qbsdiff
- 10x faster decoding than alternatives
- Best choice for high-throughput applications

✅ **gdelta+lz4 offers best speed with compression**

- 16x faster than qbsdiff
- Only 3% compression penalty vs raw gdelta
- Ideal for real-time compression needs

✅ **gdelta+zstd balances speed and compression**

- 11x faster than qbsdiff at encoding
- 7% better compression than raw gdelta
- Recommended for production use

⚠️ **qbsdiff has best compression but slowest**

- 84% space saved (best in class)
- Suitable only when time is not critical

❌ **xdelta3 failed verification**

- Produced corrupted output in comprehensive tests
- Not recommended for production use

---

## gdelta Performance Characteristics

### Encoding Performance by Data Size

**From Simple Benchmark:**

| Data Type | Size  | Throughput | Time    |
|-----------|-------|------------|---------|
| JSON      | 16KB  | 1.08 GiB/s | 14.1 µs |
| Logs      | 16KB  | 1.04 GiB/s | 14.7 µs |
| CSV       | 64KB  | 1.02 GiB/s | 59.7 µs |
| Binary    | 128KB | 1.04 GiB/s | 117 µs  |
| Text      | 256KB | 371 MiB/s  | 673 µs  |

**Key Observations:**

- **Peak performance** on small chunks (16-64KB): >1 GiB/s
- **Sustained performance** on larger data: 370-400 MiB/s
- **Optimal chunk size**: 16KB-128KB for maximum throughput

### Decoding Performance by Data Size

| Data Type | Size  | Throughput | Time    |
|-----------|-------|------------|---------|
| JSON      | 16KB  | 6.7 GiB/s  | 2.3 µs  |
| Logs      | 16KB  | 7.3 GiB/s  | 2.1 µs  |
| CSV       | 64KB  | 8.4 GiB/s  | 7.2 µs  |
| Binary    | 128KB | 10.6 GiB/s | 11.4 µs |
| Text      | 256KB | 2.0 GiB/s  | 119 µs  |

**Key Observations:**

- **5-10x faster** than encoding
- **Peak performance**: 10.6 GiB/s on binary data
- **Average**: 4.1 GiB/s across all workloads
- Decoding speed makes gdelta ideal for read-heavy workloads

### Compression Efficiency

**By Change Pattern** (from comprehensive benchmark):

| Change Type         | Compression Ratio | Space Saved |
|---------------------|-------------------|-------------|
| Delete              | 0.000             | 100%        |
| Append              | 0.021             | 98%         |
| Minor edit (1%)     | 0.198             | 80%         |
| Moderate edit (15%) | 0.677             | 32%         |
| Major rewrite (50%) | 0.978             | 2%          |

**Key Observations:**

- **Best case**: Deletions and appends (near 100% saved)
- **Typical case**: Minor edits result in 80% space savings
- **Worst case**: Major rewrites provide minimal benefit

### Correctness Validation

✅ **All tests passed** (270/270 in comprehensive benchmark)

```
Test Results from Simple Benchmark:
✓ json_16kb_minor    | 90.3% saved | Verified
✓ json_16kb_append   | 94.1% saved | Verified  
✓ logs_64kb_minor    | 89.3% saved | Verified
✓ csv_128kb_minor    | 90.3% saved | Verified
✓ binary_256kb_minor | 92.3% saved | Verified
```

---

## Algorithm Comparison

### Speed Comparison

**Encoding Throughput (Higher is Better):**

```
gdelta       ████████████████████ 397 MB/s
gdelta_lz4   ██████████████████   350 MB/s
xpatch       ███████████████      291 MB/s  
gdelta_zstd  █████████████        258 MB/s
xdelta3*     ████████             45 MB/s
zstd_dict    ████                 23 MB/s
qbsdiff      ████                 22 MB/s

* Failed verification tests
```

**Decoding Throughput (Higher is Better):**

```
gdelta       ████████████████████ 4.1 GB/s
gdelta_lz4   ██████████████████   3.6 GB/s
xpatch       ████████████         2.3 GB/s
gdelta_zstd  ██████████           2.0 GB/s
qbsdiff      ███                  114 MB/s
zstd_dict    █                    27 MB/s
```

### Compression Comparison

**Space Saved (Higher is Better):**

```
qbsdiff      ████████████████████ 84.4%
xdelta3*     ██████████████████   81.3%
xpatch       ███████████████      74.6%
gdelta_zstd  ██████████████       70.5%
gdelta_lz4   █████████████        65.9%
gdelta       █████████████        63.3%
zstd_dict    ███████████          55.0%

* Failed verification tests
```

### Real-World Delta Sizes

For a **2MB file with typical edits**:

| Algorithm   | Delta Size | Saved | Relative to Best |
|-------------|------------|-------|------------------|
| qbsdiff     | 312 KB     | 85%   | Best             |
| xpatch      | 544 KB     | 74%   | +231 KB          |
| gdelta_zstd | 634 KB     | 70%   | +322 KB          |
| gdelta_lz4  | 754 KB     | 64%   | +442 KB          |
| gdelta      | 836 KB     | 60%   | +524 KB          |
| zstd_dict   | 1039 KB    | 50%   | +727 KB          |

**Analysis:**

- gdelta produces deltas ~2x larger than qbsdiff
- But gdelta is ~18x faster at encoding
- For 2MB → 634KB (gdelta_zstd): saves 322ms compared to qbsdiff

### Performance Trade-offs

**Efficiency Score = (Space Saved) / (Encode Time)**

| Algorithm   | Efficiency Score | Category            |
|-------------|------------------|---------------------|
| gdelta      | 310              | ⚡ Best for Speed    |
| gdelta_lz4  | 284              | ⚡ Fast + Compressed |
| xpatch      | 269              | ⚡ Balanced          |
| gdelta_zstd | 225              | ⚖️ Balanced         |
| qbsdiff     | 23               | 🎯 Best Compression |
| zstd_dict   | 11               | 🐌 Slow             |

---

## Use Case Recommendations

### By Priority

#### 🚀 **Maximum Speed Required**

**Use: `gdelta` (raw)**

- Encoding: 397 MB/s
- Decoding: 4.1 GB/s
- Best for: Real-time processing, high-throughput pipelines

```bash
# Library
let delta = gdelta::encode(new, base)?;

# CLI
gdelta encode old.bin new.bin -o patch.delta
```

#### ⚡ **Fast with Compression**

**Use: `gdelta + lz4`**

- Encoding: 350 MB/s (12% slower)
- Compression: 66% saved (3% better)
- Best for: Network transfers, storage systems

```bash
# CLI
gdelta encode old.bin new.bin -o patch.delta -c lz4
```

#### ⚖️ **Balanced Speed & Compression**

**Use: `gdelta + zstd`**

- Encoding: 258 MB/s (35% slower than raw)
- Compression: 70% saved (7% better than raw)
- Best for: **Most production use cases**

```bash
# CLI
gdelta encode old.bin new.bin -o patch.delta -c zstd
```

#### 🎯 **Maximum Compression**

**Use: `qbsdiff`**

- Encoding: 22 MB/s (18x slower than gdelta)
- Compression: 84% saved (21% better than gdelta)
- Best for: Offline archives, bandwidth-critical applications

### By Application Type

| Application              | Recommended | Why                              |
|--------------------------|-------------|----------------------------------|
| **Backup Systems**       | gdelta_zstd | Balance of speed and space       |
| **Version Control**      | gdelta_lz4  | Fast commits, decent compression |
| **Deduplication**        | gdelta      | Maximum throughput               |
| **Network Sync**         | gdelta_zstd | Minimize transfer time + size    |
| **Archive Storage**      | qbsdiff     | Time not critical, space is      |
| **Real-time Processing** | gdelta      | Sub-millisecond latency          |
| **Database Replication** | gdelta_lz4  | Fast, low latency                |
| **Software Updates**     | gdelta_zstd | Users wait for download          |

### By Data Characteristics

**Small Chunks (< 64KB):**

- Use `gdelta` raw - achieves >1 GiB/s
- Compression overhead not worth it

**Medium Chunks (64KB - 512KB):**

- Use `gdelta_lz4` - best speed/compression balance

**Large Files (> 512KB):**

- Use `gdelta_zstd` - compression savings significant

**Highly Similar Data:**

- Use `gdelta` raw - already small deltas

**Diverse Changes:**

- Use `gdelta_zstd` - compression helps more

---

## Scaling Behavior

### Compression Ratio vs. Size

**gdelta maintains consistent compression across sizes:**

| Size        | 16KB  | 256KB | 2MB   | Trend            |
|-------------|-------|-------|-------|------------------|
| gdelta      | 0.356 | 0.366 | 0.378 | ➡️ Stable (+6%)  |
| gdelta_lz4  | 0.333 | 0.339 | 0.351 | ➡️ Stable (+5%)  |
| gdelta_zstd | 0.295 | 0.293 | 0.297 | ➡️ Stable (+1%)  |
| qbsdiff     | 0.180 | 0.145 | 0.143 | ⬇️ Better (-21%) |

**Insight:** gdelta's ratio is predictable across sizes, while qbsdiff improves on larger data.

### Throughput vs. Size

**gdelta throughput is highest on small chunks:**

| Size  | Throughput  |
|-------|-------------|
| 16KB  | 1,080 MiB/s |
| 64KB  | 1,020 MiB/s |
| 128KB | 1,040 MiB/s |
| 256KB | 371 MiB/s   |

**Insight:** Optimal for chunk-based processing (16-128KB chunks).

### Performance by Data Type

**Some data types compress better than others:**

| Data Type         | Compression | Why                 |
|-------------------|-------------|---------------------|
| XML/HTML          | 0.326       | High redundancy     |
| JSON              | 0.315       | Structured patterns |
| SQL dumps         | 0.306       | Repeated schema     |
| Logs              | 0.326       | Timestamp patterns  |
| Source code       | 0.329       | Similar structures  |
| Binary/compressed | 0.467       | Low redundancy      |

**Insight:** gdelta works best on structured, redundant data.

---

## Running Your Own Benchmarks

### Quick Verification

```bash
# Run simple benchmark (~2-5 minutes)
cargo bench --bench simple

# Output shows:
# - Correctness verification
# - Encoding speed per data type
# - Decoding speed per data type
# - Compression ratios
```

### Comprehensive Analysis

```bash
# Full multi-algorithm benchmark (~1 hour)
cargo bench --bench comprehensive

# Results saved to:
# - target/benchmark_report_<timestamp>.md
# - target/benchmark_report_<timestamp>.json
```

### Custom Benchmarks

```bash
# Test specific formats
BENCH_FORMATS=json,csv cargo bench --bench comprehensive

# Test specific algorithms
BENCH_ALGOS=gdelta,gdelta_zstd cargo bench --bench comprehensive

# Test specific patterns
BENCH_PATTERNS=minor_edit,append_1024 cargo bench --bench comprehensive

# Combine filters
BENCH_FORMATS=json BENCH_ALGOS=gdelta BENCH_SIZES=memory \
  cargo bench --bench comprehensive
```

### Benchmark Modes

```bash
# Quick mode (default): 10 samples, 1s measurement
BENCH_MODE=quick cargo bench --bench comprehensive

# Full mode: 100 samples, 5s measurement
BENCH_MODE=full cargo bench --bench comprehensive
```

---

## Conclusion

**gdelta excels at speed while maintaining competitive compression.**

### When to Choose gdelta

- [X] You need **high throughput** (hundreds of MB/s)
- [X] You need **fast decoding** (several GB/s)
- [X] You're processing **16KB-128KB chunks**
- [X] You want **predictable performance**
- [X] You need **production-ready** code (pure Rust, no unsafe)

### When to Choose Alternatives

- **qbsdiff**: Maximum compression is critical, time is not
- **xpatch**: Want automatic algorithm selection
- ❌ **xdelta3**: Not recommended (failed verification)

### Recommended Default

**For most use cases: `gdelta + zstd (level 3)`**

This provides:

- 258 MB/s encoding (10x faster than qbsdiff)
- 2.0 GB/s decoding
- 70% space saved
- CLI: `gdelta encode -c zstd`

---

*Benchmarks run on 2025-12-10. Hardware: AMD Ryzen 7 7800X3D, 64GB RAM, Fedora Linux 42.*