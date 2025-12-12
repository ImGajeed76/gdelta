# gdelta

[![Crates.io](https://img.shields.io/crates/v/gdelta.svg)](https://crates.io/crates/gdelta)
[![Documentation](https://docs.rs/gdelta/badge.svg)](https://docs.rs/gdelta)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

A fast delta compression algorithm for similar data chunks, implemented in pure Rust.

## Overview

`gdelta` is a Rust implementation of the GDelta algorithm by Haoliang Tan. It provides efficient delta encoding for
similar data chunks (typically 4KB - 64KB) commonly found in deduplication systems.

**Key Features:**

- Fast delta encoding and decoding with optional SIMD optimization
- Memory-safe implementation in pure Rust
- Simple, ergonomic API with full-featured CLI tool
- No unsafe code
- Thoroughly tested with comprehensive benchmarking suite

**Performance:**

**Synthetic benchmarks (2,097 tests):**

- **Encoding**: 496 MB/s average throughput
    - Small chunks (16KB): up to 1.08 GiB/s
    - Large data (256KB+): 370-400 MiB/s sustained
- **Decoding**: 4.4 GB/s average (9x faster than encoding)
    - Peak: 10.6 GiB/s on binary data
- **Compression**: 68% space saved (raw delta)
    - With zstd: 75% space saved, 305 MB/s
    - With lz4: 71% space saved, 430 MB/s

**Real-world git repositories (1.36M comparisons):**

- **mdn/content**: 97.0% space saved, 4µs encode, 0µs decode
- **tokio**: 94.5% space saved, 5µs encode, 0µs decode

*Benchmarked on AMD Ryzen 7 7800X3D with 16 cores (Fedora Linux 42). See [PERFORMANCE.md](PERFORMANCE.md) for detailed
analysis including comparisons with xpatch, vcdiff, qbsdiff, and zstd_dict.*

## Installation

### As a Library

Add this to your `Cargo.toml`:

```toml
[dependencies]
gdelta = "0.2"
```

### As a CLI Tool

Install using cargo:

```bash
cargo install gdelta --features cli
```

Or build from source:

```bash
git clone https://github.com/ImGajeed76/gdelta
cd gdelta
cargo build --release --features cli
# Binary at: target/release/gdelta
```

## Usage

### Library API

```rust
use gdelta::{encode, decode};

// Create base data and modified version
let base_data = b"Hello, World! This is some base data.";
let new_data = b"Hello, Rust! This is some modified data.";

// Encode the delta
let delta = encode(new_data, base_data)?;
println!("Delta size: {} bytes", delta.len());

// Decode to recover the new data
let recovered = decode(&delta, base_data)?;
assert_eq!(recovered, new_data);
```

### CLI Tool

> Use `gdelta help` to see the most up-to-date options and descriptions.

The CLI provides a simple interface for creating and applying delta patches:

**Create a delta patch:**

```bash
# Basic usage
gdelta encode old_file.bin new_file.bin -o patch.delta

# With compression (recommended)
gdelta encode old_file.bin new_file.bin -o patch.delta -c zstd
gdelta encode old_file.bin new_file.bin -o patch.delta -c lz4

# With verification
gdelta encode old_file.bin new_file.bin -o patch.delta --verify
```

**Apply a delta patch:**

```bash
# Auto-detects compression format
gdelta decode old_file.bin patch.delta -o new_file.bin

# Force specific format (if magic bytes conflict)
gdelta decode old_file.bin patch.delta -o new_file.bin --format zstd
```

**Options:**

- `-c, --compress <FORMAT>` - Compression: none, zstd, lz4 (default: none)
- `-v, --verify` - Verify delta after creation (encode only)
- `-y, --yes` - Skip memory warning prompts
- `-f, --force` - Overwrite existing files
- `-q, --quiet` - Suppress output except errors

**Example workflow:**

```bash
# Create compressed delta
gdelta encode database-v1.db database-v2.db -o update.delta -c zstd

# Later, apply the update
gdelta decode database-v1.db update.delta -o database-v2-restored.db

# Verify the result
diff database-v2.db database-v2-restored.db
```

**Memory Management:**

The CLI monitors memory usage and warns when operations might use >80% of available RAM. This is important because
gdelta loads entire files into memory.

```bash
# For large files, the tool will prompt:
⚠  Memory warning: This operation requires ~12.4 GB
  Available: 8.2 GB free (16 GB total)
  
  Continue? [y/N]: 
```

Use `-y` to skip prompts in automated scripts.

## How It Works

GDelta uses:

1. **GEAR Rolling Hash** - Fast fingerprinting for chunk boundaries
2. **Variable-Length Integer Encoding** - Efficient space utilization
3. **Copy/Literal Instructions** - Minimal delta representation
4. **Prefix/Suffix Matching** - Optimized for common data patterns

The algorithm identifies matching regions between base and new data, then encodes only the differences as a series of
copy and literal instructions.

## Algorithm Parameters

The implementation uses optimized default parameters:

- Chunk size: 300 KB
- Word size: 8 bytes
- Base sample rate: 3
- Features: Skip optimization, reverse matching

These parameters are tuned for typical deduplication workloads.

## Comparison with Other Algorithms

### Synthetic Workloads (2,097 tests)

| Algorithm       | Speed    | Compression | Memory | Use Case                      |
|-----------------|----------|-------------|--------|-------------------------------|
| **gdelta**      | 496 MB/s | 68%         | Low    | Maximum speed                 |
| **gdelta+lz4**  | 430 MB/s | 71%         | Low    | Fast with compression         |
| **gdelta+zstd** | 305 MB/s | 75%         | Low    | Balanced speed/compression    |
| **xpatch**      | 306 MB/s | 75%         | Low    | Automatic algorithm selection |
| **vcdiff**      | 94 MB/s  | 64%         | Medium | Standard delta format         |
| **qbsdiff**     | 22 MB/s  | 84%         | Medium | Maximum compression           |
| **zstd_dict**   | 14 MB/s  | 55%         | Medium | Dictionary-based compression  |

### Git Repository Workloads (1.36M comparisons)

**mdn/content repository (306K comparisons):**

| Algorithm     | Compression | Encode Time | Notes            |
|---------------|-------------|-------------|------------------|
| xpatch (tags) | 97.5% saved | 319 µs      | Best compression |
| xpatch (seq)  | 97.5% saved | 14 µs       | Fast alternative |
| **gdelta**    | 97.0% saved | 4 µs        | **Fastest**      |
| vcdiff        | 95.6% saved | 49 µs       | Standard format  |

**tokio repository (33K comparisons):**

| Algorithm     | Compression | Encode Time | Notes            |
|---------------|-------------|-------------|------------------|
| xpatch (tags) | 97.9% saved | 306 µs      | Best compression |
| xpatch (seq)  | 95.6% saved | 24 µs       | Balanced         |
| **gdelta**    | 94.5% saved | 5 µs        | **Fastest**      |
| vcdiff        | 93.1% saved | 28 µs       | Standard format  |

*Git benchmark data from [xpatch test results](https://github.com/ImGajeed76/xpatch/tree/master/test_results).
See [PERFORMANCE.md](PERFORMANCE.md) for detailed analysis.*

### Key Observations

**Synthetic benchmarks:**

- qbsdiff: best compression (84%), slowest speed (22 MB/s)
- gdelta: fastest speed (496 MB/s), competitive compression (68%)
- xpatch/gdelta+zstd: balanced (75% compression, 305 MB/s)
- Algorithm choice impacts compression by 29 percentage points

**Git repositories:**

- All algorithms achieve 93-98% compression on real file evolution
- xpatch with tag optimization: best compression (97.5-97.9%)
- gdelta: fastest (4-5µs), competitive compression (94.5-97.0%)
- Differences measured in microseconds, all are fast

**Workload matters:** Performance characteristics vary significantly between synthetic edits and real file evolution
patterns.

## Use Cases

**High-Speed Applications:**

- Deduplication systems: gdelta (496 MB/s)
- Real-time processing: gdelta (4.4 GB/s decode)
- Database replication: gdelta or gdelta+lz4

**Version Control Systems:**

- Best compression: xpatch with tags (97.5-97.9% on real repos)
- Fastest: gdelta (4-5µs per operation)
- Balanced: xpatch sequential mode or gdelta+lz4

**Backup & Storage:**

- Space-critical: qbsdiff (84% saved)
- Balanced: gdelta+zstd (75% saved, 305 MB/s)
- Fast backups: gdelta or gdelta+lz4

**Network Synchronization:**

- Low bandwidth: qbsdiff or xpatch
- Low latency: gdelta (4.4 GB/s decode)
- Balanced: gdelta+zstd

**Standards Compliance:**

- RFC 3284 (VCDIFF): vcdiff implementation

## Development

### Running Tests

```bash
# Run unit tests
cargo test

# Run integration tests
cargo test --test '*'

# Run CLI test suite
./test_gdelta.sh

# Run simple benchmarks (quick verification)
cargo bench --bench simple

# Run comprehensive benchmarks (15-30 min)
cargo bench --bench comprehensive

# Run comprehensive benchmarks with custom filters
BENCH_FORMATS=json,csv BENCH_ALGOS=gdelta,xpatch cargo bench --bench comprehensive
```

### Benchmark Modes

The comprehensive benchmark supports two modes:

```bash
# Quick mode (default): smaller sample size, faster
BENCH_MODE=quick cargo bench --bench comprehensive

# Full mode: larger sample size, more accurate
BENCH_MODE=full cargo bench --bench comprehensive
```

Results are saved to `target/benchmark_report_<timestamp>.md` and `.json`.

## Credits

This is a Rust implementation of the GDelta algorithm by **Haoliang Tan**.

Original repositories/resources:

- [GDelta Paper](https://ieeexplore.ieee.org/abstract/document/9229609/)
- [Original GDelta (C++)](https://github.com/apple-ouyang/gdelta)
- [GDelta with ZSTD (C++)](https://github.com/AnsonHooL/Gdelta)

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

---

Built with ❤️ by [ImGajeed76](https://github.com/ImGajeed76)