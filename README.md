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
- Simple, ergonomic API with CLI tool
- No unsafe code
- Thoroughly tested

**Performance:**

- **Encoding**: 900-1,000 MiB/s (~1 GB/s) - up to 19% faster with SIMD
- **Decoding**: 5-9 GB/s (5-8x faster than encoding)
- Faster than Xdelta, Zdelta, Ddelta, and Edelta
- Optimized for inter-chunk redundancy removal
- Best used with general compression (e.g., ZSTD) for additional compression

*Benchmarked on 11th Gen Intel® Core™ i7-11370H with similar data chunks*

## Installation

### As a Library

Add this to your `Cargo.toml`:

```toml
[dependencies]
gdelta = "0.1"
```

### As a CLI Tool

Install using cargo:

```bash
cargo install gdelta --features cli
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
⚠ Memory warning: This operation requires ~12.4 GB
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

## Comparison with Other Delta Algorithms

| Feature     | GDelta | Xdelta    | Zdelta |
|-------------|--------|-----------|--------|
| Speed       | Fast   | Medium    | Slow   |
| Memory      | Low    | Medium    | High   |
| Compression | Good*  | Excellent | Good   |

*Use with ZSTD/LZ4 for best compression ratio (depending on file size)

## Use Cases

- **Deduplication Systems** - Compress similar chunks
- **Backup Software** - Incremental backups
- **File Synchronization** - Minimize transfer size
- **Version Control** - Efficient diff storage
- **Binary Patching** - Software updates and distributions

## Development

### Running Tests

```bash
# Run unit tests
cargo test

# Run CLI test suite
./test_gdelta.sh

# Run benchmarks
cargo bench
```

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