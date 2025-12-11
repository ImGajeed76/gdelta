# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.1] - 2025-12-11

### Fixed
- **Critical Bug**: Fixed incorrect suffix offset calculation in trivial case encoding
  - When `prefix_size + suffix_size >= base_size`, the suffix was incorrectly copied from `new_data` offset instead of `base_data` offset
  - This caused "Copy offset exceeds base size" errors during decode when new data size > base data size
  - Affected real-world scenarios with growing files (e.g., appending to source code files)
  - Now correctly uses `base_size - suffix_size` as the copy offset for suffix in trivial case

## [0.2.0] - 2025-12-11

### Added
- **CLI Tool**: Full-featured command-line interface for creating and applying delta patches
    - Compression support (none, zstd, lz4) with auto-detection
    - Memory usage warnings and monitoring
    - Verification mode for encode operations
    - Force overwrite and quiet modes
    - Colorized output with progress indicators
- **Comprehensive Benchmark Suite**: Multi-algorithm comparison framework
    - Tests 7 delta compression algorithms (gdelta, xpatch, xdelta3, qbsdiff, and variants)
    - 15 realistic data formats (JSON, XML, CSV, logs, source code, etc.)
    - 7 change patterns (minor edits, major rewrites, appends, etc.)
    - Multiple data sizes (16KB to 2MB)
    - Generates detailed markdown and JSON reports
    - WAL-based metrics collection with graceful Ctrl+C handling
- **CLI Test Suite**: Bash script with 40+ integration tests covering all CLI features
- **Benchmark Documentation**: Simple benchmark suite for quick performance validation

### Changed
- **Documentation**: Extensively updated README with CLI usage examples and memory management guidance
- **Project Structure**: Reorganized with separate binary crate for CLI tool
- **Performance Claims**: Updated with actual benchmarked data across multiple scenarios

### Performance
- **Verified Performance** (from comprehensive benchmarks):
    - Encoding: 370-400 MB/s average across diverse workloads
    - Decoding: 4.1 GB/s average (10x faster than encoding)
    - Peak encoding: 1.08 GiB/s on cache-friendly data (16KB chunks)
    - Peak decoding: 10.6 GiB/s on binary data
- **Compression Efficiency**: 63.3% space saved on average (raw delta, before compression)
- **With Compression**:
    - gdelta+zstd: 70.5% space saved, 258 MB/s encode
    - gdelta+lz4: 65.9% space saved, 350 MB/s encode

### Comparison Results
- **Fastest Encoding**: gdelta (397 MB/s) > gdelta_lz4 (350 MB/s) > xpatch (291 MB/s)
- **Fastest Decoding**: gdelta (4.1 GB/s) > gdelta_lz4 (3.6 GB/s) > xpatch (2.3 GB/s)
- **Best Compression**: qbsdiff (84.4%) > xpatch (74.6%) > gdelta_zstd (70.5%)
- **Best Overall**: gdelta offers best speed-to-compression balance for most use cases

## [0.1.1] - 2025-12-04

### Added
- Optional SIMD optimization using the `wide` crate
- SIMD feature enabled by default for improved performance

### Performance
- 15-19% faster encoding on similar data with SIMD
- 6-12% faster encoding on general data
- SIMD can be disabled with `--no-default-features` if needed

### Changed
- Optimized `find_common_prefix`, `find_common_suffix`, and `extend_match` functions

## [0.1.0] - 2025-12-04

### Added
- Initial release
- Fast delta encoding and decoding
- Pure Rust implementation with no unsafe code
- Comprehensive test suite
- Documentation and examples