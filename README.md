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

- **Encoding**: 370-1,080 MiB/s depending on data characteristics
    - Small chunks (16KB): up to 1.08 GiB/s
    - Large data (256KB+): 370-400 MiB/s sustained
- **Decoding**: 2.0-10.6 GiB/s (5-28x faster than encoding)
    - Average: 4.1 GiB/s across diverse workloads
- **Compression**: 63% space saved on average (raw delta)
    - With zstd: 70% space saved, 258 MiB/s
    - With lz4: 66% space saved, 350 MiB/s
- **Fastest in class**: Among open-source delta algorithms, gdelta offers the best speed-to-compression balance

*Benchmarked on AMD Ryzen 7 7800X3D with 16 cores (Fedora Linux 42). See [PERFORMANCE.md](PERFORMANCE.md) for detailed analysis.*

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

**Performance Comparison** (from comprehensive benchmarks):

| Algorithm       | Speed    | Compression | Memory | Use Case                      |
|-----------------|----------|-------------|--------|-------------------------------|
| **gdelta**      | 397 MB/s | 63%         | Low    | Best all-around speed         |
| **gdelta+zstd** | 258 MB/s | 70%         | Low    | Balanced speed/compression    |
| **gdelta+lz4**  | 350 MB/s | 66%         | Low    | Fast with compression         |
| **xpatch**      | 291 MB/s | 75%         | Low    | Automatic algorithm selection |
| **qbsdiff**     | 22 MB/s  | 84%         | Medium | Maximum compression           |
| **xdelta3**     | 45 MB/s  | 81%         | Medium | **Failed verification** ⚠️    |

*See [PERFORMANCE.md](PERFORMANCE.md) for detailed benchmarks and methodology.*

**Key Takeaways:**
- **Fastest**: gdelta and gdelta+lz4 for high-speed applications
- **Best Compression**: qbsdiff if speed is not critical
- **Best Balance**: gdelta+zstd for most production use cases
- **Decoding**: gdelta is 2-5x faster at decoding than alternatives

## Use Cases

- **Deduplication Systems** - Compress similar chunks
- **Backup Software** - Incremental backups
- **File Synchronization** - Minimize transfer size
- **Version Control** - Efficient diff storage
- **Binary Patching** - Software updates and distributions
- **Cloud Storage** - Reduce storage costs
- **Database Replication** - Minimize bandwidth

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