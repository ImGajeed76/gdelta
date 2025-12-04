# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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