//! Comprehensive benchmark suite for delta compression algorithms.
//!
//! Features:
//! - Multiple realistic data patterns (JSON, XML, CSV, logs, images, etc.)
//! - Test from different memory sources (cache, RAM, disk)
//! - Measure speed, throughput, and compression ratio
//! - Verify reconstruction correctness
//! - WAL-based metrics collection
//! - generate Markdown and JSON reports
//! - Graceful Ctrl+C handling with partial results
//!
//! Run: cargo bench --bench comprehensive
//! Quick mode: `BENCH_MODE=quick` cargo bench --bench comprehensive
//! Full mode: `BENCH_MODE=full` cargo bench --bench comprehensive
//! Custom: `BENCH_ALGOS=gdelta,xpatch` `BENCH_FORMATS=json,csv` cargo bench --bench comprehensive
//! View report: cat `target/benchmark_report.md`

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use fake::Fake;
use fake::faker::internet::en::SafeEmail;
use fake::faker::lorem::en::{Sentence, Paragraph};
use fake::faker::name::en::Name;
use gdelta::{decode, encode};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions, create_dir_all};
use std::hint::black_box;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use sysinfo::System;
use std::cmp::Ordering as CmpOrdering;

// ============================================================================
// Configuration
// ============================================================================

fn get_timestamp() -> String {
    chrono::Local::now().format("%Y%m%d_%H%M%S").to_string()
}

fn get_wal_file(timestamp: &str) -> String {
    format!("target/benchmark_results_{timestamp}/metrics.wal")
}

fn get_report_md(timestamp: &str) -> String {
    format!("target/benchmark_report_{timestamp}.md")
}

fn get_report_json(timestamp: &str) -> String {
    format!("target/benchmark_report_{timestamp}.json")
}

// Global flag for graceful shutdown
static SHUTDOWN_FLAG: AtomicBool = AtomicBool::new(false);

// ============================================================================
// Signal Handling
// ============================================================================

fn setup_signal_handler() {
    ctrlc::set_handler(move || {
        println!("\n\n🛑 Received Ctrl+C, finishing current test and generating reports...\n");
        SHUTDOWN_FLAG.store(true, Ordering::SeqCst);
    })
    .expect("Error setting Ctrl-C handler");
}

fn should_continue() -> bool {
    !SHUTDOWN_FLAG.load(Ordering::SeqCst)
}

// ============================================================================
// Algorithm Trait
// ============================================================================

trait DeltaAlgorithm: Send + Sync {
    fn name(&self) -> &str;
    fn encode(&self, new: &[u8], base: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>>;
    fn decode(&self, delta: &[u8], base: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>>;
}

struct GdeltaAlgorithm;

impl DeltaAlgorithm for GdeltaAlgorithm {
    fn name(&self) -> &'static str {
        "gdelta"
    }

    fn encode(&self, new: &[u8], base: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        encode(new, base).map_err(std::convert::Into::into)
    }

    fn decode(&self, delta: &[u8], base: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        decode(delta, base).map_err(std::convert::Into::into)
    }
}

// Gdelta with Zstd compression
struct GdeltaZstdAlgorithm;

impl DeltaAlgorithm for GdeltaZstdAlgorithm {
    fn name(&self) -> &'static str {
        "gdelta_zstd"
    }

    fn encode(&self, new: &[u8], base: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let delta = encode(new, base)?;
        let compressed = zstd::encode_all(&delta[..], 3)?; // Level 3 for speed
        Ok(compressed)
    }

    fn decode(&self, delta: &[u8], base: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let decompressed = zstd::decode_all(delta)?;
        decode(&decompressed, base).map_err(std::convert::Into::into)
    }
}

// Gdelta with LZ4 compression
struct GdeltaLz4Algorithm;

impl DeltaAlgorithm for GdeltaLz4Algorithm {
    fn name(&self) -> &'static str {
        "gdelta_lz4"
    }

    #[allow(clippy::cast_possible_truncation)]
    fn encode(&self, new: &[u8], base: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let delta = encode(new, base)?;

        // Compress with LZ4
        let compressed = lz4::block::compress(&delta, None, false)?;

        // Prepend the original size (needed for decompression)
        let mut result = Vec::new();
        result.extend_from_slice(&(delta.len() as u32).to_le_bytes());
        result.extend_from_slice(&compressed);

        Ok(result)
    }

    #[allow(clippy::cast_possible_truncation)]
    #[allow(clippy::cast_possible_wrap)]
    fn decode(&self, delta: &[u8], base: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        // Extract the original size
        if delta.len() < 4 {
            return Err("Invalid LZ4 delta: too short".into());
        }

        let original_size = u32::from_le_bytes([delta[0], delta[1], delta[2], delta[3]]) as usize;
        let compressed_data = &delta[4..];

        // Decompress with size hint
        let decompressed = lz4::block::decompress(compressed_data, Some(original_size as i32))?;

        // Apply delta
        decode(&decompressed, base).map_err(std::convert::Into::into)
    }
}

// XPatch (uses gdelta internally with automatic algorithm selection)
struct XpatchAlgorithm;

impl DeltaAlgorithm for XpatchAlgorithm {
    fn name(&self) -> &'static str {
        "xpatch"
    }

    fn encode(&self, new: &[u8], base: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let tag = 0; // No metadata needed for benchmarking
        let enable_zstd = true; // Enable for better compression
        Ok(xpatch::delta::encode(tag, base, new, enable_zstd))
    }

    fn decode(&self, delta: &[u8], base: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        xpatch::delta::decode(base, delta).map_err(std::convert::Into::into)
    }
}

// xdelta3

struct Xdelta3Algorithm;

impl DeltaAlgorithm for Xdelta3Algorithm {
    fn name(&self) -> &'static str {
        "xdelta3"
    }

    fn encode(&self, new: &[u8], base: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        xdelta3::encode(base, new).ok_or_else(|| "xdelta3 encode failed".into())
    }

    fn decode(&self, delta: &[u8], base: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        xdelta3::decode(delta, base).ok_or_else(|| "xdelta3 decode failed".into())
    }
}

// qbsdiff - industry standard
struct QbsdiffAlgorithm;

impl DeltaAlgorithm for QbsdiffAlgorithm {
    fn name(&self) -> &'static str {
        "qbsdiff"
    }

    fn encode(&self, new: &[u8], base: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut patch = Vec::new();
        qbsdiff::Bsdiff::new(base, new).compare(std::io::Cursor::new(&mut patch))?;
        Ok(patch)
    }

    fn decode(&self, delta: &[u8], base: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let patcher = qbsdiff::Bspatch::new(delta)?;
        let mut target = Vec::new();
        patcher.apply(base, std::io::Cursor::new(&mut target))?;
        Ok(target)
    }
}

// zstd dictionary
struct ZstdDictAlgorithm;

impl DeltaAlgorithm for ZstdDictAlgorithm {
    fn name(&self) -> &'static str {
        "zstd_dict"
    }

    fn encode(&self, new: &[u8], base: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        // Train dictionary from base file split into chunks
        let chunk_size = base.len().min(10000);
        let samples: Vec<&[u8]> = base.chunks(chunk_size).collect();

        let dict = zstd::dict::from_samples(&samples, 100_000)?;

        // Compress new file using the dictionary
        let mut compressor = zstd::bulk::Compressor::with_dictionary(3, &dict)?;
        Ok(compressor.compress(new)?)
    }

    fn decode(&self, delta: &[u8], base: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        // Train same dictionary from base
        let chunk_size = base.len().min(10000);
        let samples: Vec<&[u8]> = base.chunks(chunk_size).collect();

        let dict = zstd::dict::from_samples(&samples, 100_000)?;

        // Decompress using the dictionary
        let estimated_size = delta.len() * 10; // Higher estimate
        let mut decompressor = zstd::bulk::Decompressor::with_dictionary(&dict)?;
        Ok(decompressor.decompress(delta, estimated_size)?)
    }
}

// ============================================================================
// Realistic Data generators
// ============================================================================

#[derive(Clone, Copy, Debug)]
enum DataFormat {
    /// JSON data (API responses, configs)
    Json,
    /// XML data (documents, configs)
    Xml,
    /// CSV data (spreadsheets, exports)
    Csv,
    /// Log files (application logs)
    Logs,
    /// Source code (various languages)
    SourceCode,
    /// Markdown/documentation
    Markdown,
    /// SQL dumps
    SqlDump,
    /// Binary protocol buffers
    Protobuf,
    /// Compressed data (simulated)
    Compressed,
    /// Image data (bitmap-like)
    ImageData,
    /// Database pages (mixed binary)
    DatabasePage,
    /// Email/MIME
    Email,
    /// HTML
    Html,
    /// YAML config
    Yaml,
    /// Plain text
    PlainText,
}

impl DataFormat {
    fn name(&self) -> &str {
        match self {
            DataFormat::Json => "json",
            DataFormat::Xml => "xml",
            DataFormat::Csv => "csv",
            DataFormat::Logs => "logs",
            DataFormat::SourceCode => "source_code",
            DataFormat::Markdown => "markdown",
            DataFormat::SqlDump => "sql_dump",
            DataFormat::Protobuf => "protobuf",
            DataFormat::Compressed => "compressed",
            DataFormat::ImageData => "image_data",
            DataFormat::DatabasePage => "database_page",
            DataFormat::Email => "email",
            DataFormat::Html => "html",
            DataFormat::Yaml => "yaml",
            DataFormat::PlainText => "plain_text",
        }
    }

    fn generate(self, size_target: usize) -> Vec<u8> {
        let mut rng = StdRng::seed_from_u64(42);

        match self {
            DataFormat::Json => generate_json(size_target, &mut rng),
            DataFormat::Xml => generate_xml(size_target, &mut rng),
            DataFormat::Csv => generate_csv(size_target, &mut rng),
            DataFormat::Logs => generate_logs(size_target, &mut rng),
            DataFormat::SourceCode => generate_source_code(size_target, &mut rng),
            DataFormat::Markdown => generate_markdown(size_target, &mut rng),
            DataFormat::SqlDump => generate_sql_dump(size_target, &mut rng),
            DataFormat::Protobuf => generate_protobuf_like(size_target, &mut rng),
            DataFormat::Compressed => generate_compressed_like(size_target, &mut rng),
            DataFormat::ImageData => generate_image_data(size_target, &mut rng),
            DataFormat::DatabasePage => generate_database_page(size_target, &mut rng),
            DataFormat::Email => generate_email(size_target, &mut rng),
            DataFormat::Html => generate_html(size_target, &mut rng),
            DataFormat::Yaml => generate_yaml(size_target, &mut rng),
            DataFormat::PlainText => generate_plain_text(size_target, &mut rng),
        }
    }
}

fn generate_json(size_target: usize, rng: &mut StdRng) -> Vec<u8> {
    let mut data = String::from("[\n");

    while data.len() < size_target {
        let name: String = Name().fake_with_rng(rng);
        let email: String = SafeEmail().fake_with_rng(rng);
        let id: u32 = rng.random_range(1000..99999);

        data.push_str(format!(
            "  {{\"id\": {}, \"name\": \"{}\", \"email\": \"{}\", \"active\": {}}},\n",
            id,
            name,
            email,
            rng.random_bool(0.8)
        ).as_str());
    }

    data.push_str("]\n");
    data.into_bytes()
}

fn generate_xml(size_target: usize, rng: &mut StdRng) -> Vec<u8> {
    let mut data = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<root>\n");

    while data.len() < size_target {
        let name: String = Name().fake_with_rng(rng);
        let content: String = Sentence(3..10).fake_with_rng(rng);

        data.push_str(format!(
            "  <item id=\"{}\">\n    <name>{}</name>\n    <content>{}</content>\n  </item>\n",
            rng.random_range(1000..99999),
            name,
            content
        ).as_str());
    }

    data.push_str("</root>\n");
    data.into_bytes()
}

fn generate_csv(size_target: usize, rng: &mut StdRng) -> Vec<u8> {
    let mut data = String::from("id,name,email,timestamp,value\n");

    while data.len() < size_target {
        let name: String = Name().fake_with_rng(rng);
        let email: String = SafeEmail().fake_with_rng(rng);
        let timestamp = 1_700_000_000 + rng.random_range(0..10_000_000);
        let value = rng.random_range(0.0..1000.0);

        data.push_str(format!(
            "{},{},{},{},{:.2}\n",
            rng.random_range(1000..99999),
            name,
            email,
            timestamp,
            value
        ).as_str());
    }

    data.into_bytes()
}

fn generate_logs(size_target: usize, rng: &mut StdRng) -> Vec<u8> {
    let mut data = String::new();
    let levels = ["INFO", "WARN", "ERROR", "DEBUG"];

    while data.len() < size_target {
        let level = levels[rng.random_range(0..levels.len())];
        let timestamp = 1_700_000_000 + rng.random_range(0..10_000_000);
        let message: String = Sentence(5..15).fake_with_rng(rng);

        data.push_str(format!(
            "[{}] {} [thread-{}] {}\n",
            timestamp,
            level,
            rng.random_range(1..20),
            message
        ).as_str());
    }

    data.into_bytes()
}

fn generate_source_code(size_target: usize, rng: &mut StdRng) -> Vec<u8> {
    let mut data = String::from("fn main() {\n");

    while data.len() < size_target {
        let var_name = format!("var_{}", rng.random_range(0..100));
        let value = rng.random_range(0..1000);

        data.push_str(format!("    let {var_name} = {value};\n").as_str());

        if rng.random_bool(0.3) {
            data.push_str("    if condition {\n        do_something();\n    }\n");
        }
    }

    data.push_str("}\n");
    data.into_bytes()
}

fn generate_markdown(size_target: usize, rng: &mut StdRng) -> Vec<u8> {
    let mut data = String::from("# Document Title\n\n");

    while data.len() < size_target {
        data.push_str(format!("## Section {}\n\n", rng.random_range(1..100)).as_str());

        let paragraph: String = Paragraph(3..8).fake_with_rng(rng);
        data.push_str(&paragraph);
        data.push_str("\n\n");

        if rng.random_bool(0.4) {
            data.push_str("```rust\nfn example() {\n    println!(\"Hello\");\n}\n```\n\n");
        }
    }

    data.into_bytes()
}

fn generate_sql_dump(size_target: usize, rng: &mut StdRng) -> Vec<u8> {
    let mut data = String::from("-- SQL Dump\n\n");

    while data.len() < size_target {
        let name: String = Name().fake_with_rng(rng);
        let email: String = SafeEmail().fake_with_rng(rng);

        data.push_str(format!(
            "INSERT INTO users (id, name, email) VALUES ({}, '{}', '{}');\n",
            rng.random_range(1000..99999),
            name,
            email
        ).as_str());
    }

    data.into_bytes()
}

fn generate_protobuf_like(size_target: usize, rng: &mut StdRng) -> Vec<u8> {
    let mut data = Vec::new();

    while data.len() < size_target {
        // Simulated protobuf: field tags, varints, length-delimited strings
        data.push(0x08); // field 1, varint
        data.push(rng.random_range(0..128));

        data.push(0x12); // field 2, length-delimited
        let str_len = rng.random_range(5..50);
        data.push(str_len);
        data.extend(std::iter::repeat_with(|| rng.random::<u8>()).take(str_len as usize));
    }

    data
}

fn generate_compressed_like(size_target: usize, rng: &mut StdRng) -> Vec<u8> {
    // High entropy data (simulating already compressed data)
    (0..size_target).map(|_| rng.random()).collect()
}

#[allow(clippy::cast_possible_truncation)]
#[allow(clippy::cast_sign_loss)]
fn generate_image_data(size_target: usize, rng: &mut StdRng) -> Vec<u8> {
    let mut data = Vec::new();

    // Simulate bitmap with some patterns
    while data.len() < size_target {
        let pixel = [
            rng.random_range(0..256) as u8,
            rng.random_range(0..256) as u8,
            rng.random_range(0..256) as u8,
            255,
        ];
        data.extend_from_slice(&pixel);
    }

    data.truncate(size_target);
    data
}

fn generate_database_page(size_target: usize, rng: &mut StdRng) -> Vec<u8> {
    let mut data = Vec::new();

    // Simulate database page structure
    while data.len() < size_target {
        // Page header
        data.extend_from_slice(&[0xFF, 0xFE, 0x00, 0x01]);

        // Records with varying lengths
        for _ in 0..rng.random_range(5..20) {
            let record_len = rng.random_range(20..100);
            data.extend(std::iter::repeat_with(|| rng.random::<u8>()).take(record_len));
        }

        // Padding
        let padding = rng.random_range(0..50);
        data.extend(std::iter::repeat_n(0u8, padding));
    }

    data.truncate(size_target);
    data
}

fn generate_email(size_target: usize, rng: &mut StdRng) -> Vec<u8> {
    let mut data = String::from("From: sender@example.com\nTo: recipient@example.com\n");
    data.push_str("Subject: Test Email\nDate: Mon, 1 Jan 2024 12:00:00 +0000\n\n");

    while data.len() < size_target {
        let paragraph: String = Paragraph(5..10).fake_with_rng(rng);
        data.push_str(&paragraph);
        data.push_str("\n\n");
    }

    data.into_bytes()
}

fn generate_html(size_target: usize, rng: &mut StdRng) -> Vec<u8> {
    let mut data =
        String::from("<!DOCTYPE html>\n<html>\n<head><title>Page</title></head>\n<body>\n");

    while data.len() < size_target {
        let title: String = Sentence(3..8).fake_with_rng(rng);
        let content: String = Paragraph(3..6).fake_with_rng(rng);

        data.push_str(format!(
            "<div class=\"item\">\n  <h2>{title}</h2>\n  <p>{content}</p>\n</div>\n"
        ).as_str());
    }

    data.push_str("</body>\n</html>\n");
    data.into_bytes()
}

fn generate_yaml(size_target: usize, rng: &mut StdRng) -> Vec<u8> {
    let mut data = String::from("config:\n");

    while data.len() < size_target {
        let key = format!("setting_{}", rng.random_range(0..100));
        let value = rng.random_range(0..1000);

        data.push_str(format!("  {key}: {value}\n").as_str());

        if rng.random_bool(0.3) {
            data.push_str("  nested:\n    - item1\n    - item2\n");
        }
    }

    data.into_bytes()
}

fn generate_plain_text(size_target: usize, rng: &mut StdRng) -> Vec<u8> {
    let mut data = String::new();

    while data.len() < size_target {
        let paragraph: String = Paragraph(5..12).fake_with_rng(rng);
        data.push_str(&paragraph);
        data.push_str("\n\n");
    }

    data.into_bytes()
}

// ============================================================================
// Change Patterns
// ============================================================================

#[derive(Clone, Copy, Debug)]
enum ChangePattern {
    /// Minor edits (1-5% of content changed)
    MinorEdit,
    /// Moderate changes (10-20% changed)
    ModerateEdit,
    /// Major rewrite (40-60% changed)
    MajorRewrite,
    /// Append only (add new data)
    Append(usize),
    /// Insert in middle
    Insert { position_pct: f32, size: usize },
    /// Delete sections
    Delete { position_pct: f32, size: usize },
    /// Line-based changes (for text)
    LineChanges { pct: f32 },
}

impl ChangePattern {
    #[allow(clippy::cast_possible_truncation)]
    #[allow(clippy::cast_sign_loss)]
    fn name(&self) -> String {
        match self {
            ChangePattern::MinorEdit => "minor_edit".to_string(),
            ChangePattern::ModerateEdit => "moderate_edit".to_string(),
            ChangePattern::MajorRewrite => "major_rewrite".to_string(),
            ChangePattern::Append(n) => format!("append_{n}"),
            ChangePattern::Insert { position_pct, size } => {
                format!("insert_{}pct_{}", (position_pct * 100.0) as u32, size)
            }
            ChangePattern::Delete { position_pct, size } => {
                format!("delete_{}pct_{}", (position_pct * 100.0) as u32, size)
            }
            ChangePattern::LineChanges { pct } => {
                format!("line_changes_{}pct", (pct * 100.0) as u32)
            }
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    #[allow(clippy::cast_precision_loss)]
    #[allow(clippy::cast_sign_loss)]
    fn apply(&self, base: &[u8]) -> Vec<u8> {
        let mut rng = StdRng::seed_from_u64(123);

        match self {
            ChangePattern::MinorEdit => {
                let mut new = base.to_vec();
                let changes = (base.len() as f32 * 0.02) as usize;
                for _ in 0..changes {
                    if new.is_empty() {
                        break;
                    }
                    let idx = rng.random_range(0..new.len());
                    new[idx] = rng.random();
                }
                new
            }
            ChangePattern::ModerateEdit => {
                let mut new = base.to_vec();
                let changes = (base.len() as f32 * 0.15) as usize;
                for _ in 0..changes {
                    if new.is_empty() {
                        break;
                    }
                    let idx = rng.random_range(0..new.len());
                    new[idx] = rng.random();
                }
                new
            }
            ChangePattern::MajorRewrite => {
                let mut new = base.to_vec();
                let changes = (base.len() as f32 * 0.50) as usize;
                for _ in 0..changes {
                    if new.is_empty() {
                        break;
                    }
                    let idx = rng.random_range(0..new.len());
                    new[idx] = rng.random();
                }
                new
            }
            ChangePattern::Append(size) => {
                let mut new = base.to_vec();
                new.extend(std::iter::repeat_with(|| rng.random::<u8>()).take(*size));
                new
            }
            ChangePattern::Insert { position_pct, size } => {
                let mut new = base.to_vec();
                if new.is_empty() {
                    return new;
                }
                let pos = ((base.len() as f32 * position_pct) as usize).min(new.len());
                let insert_data: Vec<u8> = (0..*size).map(|_| rng.random()).collect();
                new.splice(pos..pos, insert_data);
                new
            }
            ChangePattern::Delete { position_pct, size } => {
                let mut new = base.to_vec();
                if new.is_empty() {
                    return new;
                }
                let pos = ((base.len() as f32 * position_pct) as usize).min(new.len());
                let end = (pos + size).min(new.len());
                if pos < end {
                    new.drain(pos..end);
                }
                new
            }
            ChangePattern::LineChanges { pct } => {
                let text = String::from_utf8_lossy(base);
                let lines: Vec<&str> = text.lines().collect();
                if lines.is_empty() {
                    return base.to_vec();
                }
                let changes = ((lines.len() as f32 * pct) as usize).max(1);

                let mut new_lines = lines.clone();
                for _ in 0..changes {
                    let idx = rng.random_range(0..new_lines.len());
                    new_lines[idx] = "MODIFIED LINE";
                }

                new_lines.join("\n").into_bytes()
            }
        }
    }
}

// ============================================================================
// Metrics and Results
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BenchmarkMetric {
    timestamp: u64,
    algorithm: String,
    data_format: String,
    change_pattern: String,
    data_source: String,
    base_size: usize,
    new_size: usize,
    delta_size: usize,
    compression_ratio: f64,
    encode_time_ns: u128,
    decode_time_ns: u128,
    verification_passed: bool,
    cache_level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HardwareInfo {
    cpu_brand: String,
    cpu_cores: usize,
    total_memory_mb: u64,
    os: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct BenchmarkReport {
    generated_at: u64,
    hardware: HardwareInfo,
    metrics: Vec<BenchmarkMetric>,
    early_termination: bool,
}

struct MetricsWal {
    path: String,
}

impl MetricsWal {
    fn new(path: &str) -> std::io::Result<Self> {
        // Extract directory from path
        if let Some(parent) = Path::new(path).parent() {
            create_dir_all(parent)?;
        }

        if Path::new(path).exists() {
            std::fs::remove_file(path)?;
        }

        Ok(Self {
            path: path.to_string(),
        })
    }

    fn append(&self, metric: &BenchmarkMetric) -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;

        let json = serde_json::to_string(metric)?;
        writeln!(file, "{json}")?;

        Ok(())
    }

    fn read_all(&self) -> std::io::Result<Vec<BenchmarkMetric>> {
        if !Path::new(&self.path).exists() {
            return Ok(Vec::new());
        }

        let file = File::open(&self.path)?;
        let reader = BufReader::new(file);
        let mut metrics = Vec::new();

        for line in reader.lines().map_while(Result::ok) {
            if let Ok(metric) = serde_json::from_str::<BenchmarkMetric>(&line) {
                metrics.push(metric);
            }
        }

        Ok(metrics)
    }
}

fn collect_hardware_info() -> HardwareInfo {
    let mut sys = System::new_all();
    sys.refresh_all();

    let cpu_brand = sys
        .cpus()
        .first().map_or_else(|| "Unknown CPU".to_string(), |cpu| cpu.brand().to_string());

    HardwareInfo {
        cpu_brand,
        cpu_cores: sys.cpus().len(),
        total_memory_mb: sys.total_memory() / 1024 / 1024,
        os: format!(
            "{} {}",
            System::name().unwrap_or_else(|| "Unknown".to_string()),
            System::os_version().unwrap_or_else(|| "Unknown".to_string())
        ),
    }
}

// ============================================================================
// Benchmark Execution
// ============================================================================

#[allow(clippy::cast_precision_loss)]
fn run_benchmark(
    algo: &dyn DeltaAlgorithm,
    format: DataFormat,
    change: ChangePattern,
    source: &str,
    cache_level: &str,
    base: &[u8],
    new: &[u8],
) -> Option<BenchmarkMetric> {
    // Encode with timeout and error handling
    let encode_start = Instant::now();
    let delta = match algo.encode(new, base) {
        Ok(d) => d,
        Err(e) => {
            eprintln!(
                "\r⚠️  {} encode failed for {} ({}): {}",
                algo.name(),
                format.name(),
                change.name(),
                e
            );
            return None;
        }
    };
    let encode_time = encode_start.elapsed();

    // Decode with error handling
    let decode_start = Instant::now();
    let reconstructed = match algo.decode(&delta[..], base) {
        Ok(r) => r,
        Err(e) => {
            eprintln!(
                "\r⚠️  {} decode failed for {} ({}): {}",
                algo.name(),
                format.name(),
                change.name(),
                e
            );
            return None;
        }
    };
    let decode_time = decode_start.elapsed();

    // Verify
    let verification_passed = reconstructed == new;

    if !verification_passed {
        eprintln!(
            "\r⚠️  {} verification failed for {} ({}): expected {} bytes, got {} bytes",
            algo.name(),
            format.name(),
            change.name(),
            new.len(),
            reconstructed.len()
        );
    }

    Some(BenchmarkMetric {
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        algorithm: algo.name().to_string(),
        data_format: format.name().to_string(),
        change_pattern: change.name(),
        data_source: source.to_string(),
        base_size: base.len(),
        new_size: new.len(),
        delta_size: delta.len(),
        compression_ratio: delta.len() as f64 / new.len() as f64,
        encode_time_ns: encode_time.as_nanos(),
        decode_time_ns: decode_time.as_nanos(),
        verification_passed,
        cache_level: cache_level.to_string(),
    })
}

// ============================================================================
// Report generation
// ============================================================================

#[allow(clippy::too_many_lines)]
#[allow(clippy::cast_precision_loss)]
fn generate_markdown_report(
    metrics: &[BenchmarkMetric],
    hardware: &HardwareInfo,
    early_termination: bool,
    output_path: &str,
) -> std::io::Result<()> {
    if metrics.is_empty() {
        println!("⚠️  No metrics to report");
        return Ok(());
    }

    let mut report = String::new();

    // Header
    report.push_str("# 🔬 Delta Compression Benchmark Report\n\n");

    if early_termination {
        report.push_str("**⚠️ PARTIAL RESULTS - Benchmark was interrupted by user**\n\n");
    }

    report.push_str(format!(
        "**Generated:** {}\n\n",
        chrono::DateTime::<chrono::Utc>::from(SystemTime::now()).format("%Y-%m-%d %H:%M:%S UTC")
    ).as_str());

    // Hardware
    report.push_str("## 💻 Hardware Configuration\n\n");
    report.push_str("````\n");
    report.push_str(format!("CPU:    {}\n", hardware.cpu_brand).as_str());
    report.push_str(format!("Cores:  {}\n", hardware.cpu_cores).as_str());
    report.push_str(format!("RAM:    {} MB\n", hardware.total_memory_mb).as_str());
    report.push_str(format!("OS:     {}\n", hardware.os).as_str());
    report.push_str("````\n\n");

    // Table of Contents
    report.push_str("## 📑 Table of Contents\n\n");
    report.push_str("1. [Executive Summary](#-executive-summary)\n");
    report.push_str("2. [Algorithm Health Status](#️-algorithm-health-status)\n");
    report.push_str("3. [Overall Rankings](#-overall-rankings)\n");
    report.push_str("4. [Performance Scaling by Size](#-performance-scaling-by-size)\n");
    report.push_str("5. [Actual Delta Sizes](#-actual-delta-sizes)\n");
    report.push_str("6. [Compression Consistency](#-compression-consistency)\n");
    report.push_str("7. [Performance by Data Format](#-performance-by-data-format)\n");
    report.push_str("8. [Performance by Change Pattern](#-performance-by-change-pattern)\n");
    report.push_str("9. [Algorithm Deep Dive](#-algorithm-deep-dive)\n");
    report.push_str("10. [Head-to-Head Comparison](#️-head-to-head-comparison)\n");
    report.push_str("11. [Speed vs Compression Trade-offs](#️-speed-vs-compression-trade-offs)\n");
    report.push_str("12. [Compression ROI Analysis](#-compression-roi-analysis)\n");
    report.push_str("13. [Quick Decision Matrix](#-quick-decision-matrix)\n");
    report.push_str("14. [Pattern-Specific Recommendations](#-pattern-specific-recommendations)\n");
    report.push_str("15. [What NOT to Use](#-what-not-to-use)\n\n");

    // Executive Summary
    report.push_str("## 📊 Executive Summary\n\n");

    let total_tests = metrics.len();
    let passed = metrics.iter().filter(|m| m.verification_passed).count();
    let failed = total_tests - passed;

    let algorithms: Vec<String> = metrics
        .iter()
        .map(|m| m.algorithm.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    report.push_str(format!("- **Total Tests:** {total_tests}\n").as_str());
    report.push_str(format!("- **Algorithms Tested:** {}\n", algorithms.len()).as_str());
    report.push_str(format!(
        "- **Verification:** {} passed, {} failed ({:.1}% success rate)\n\n",
        passed,
        failed,
        (passed as f64 / total_tests as f64) * 100.0
    ).as_str());

    // VERIFICATION STATUS
    report.push_str("## ⚠️ Algorithm Health Status\n\n");
    report.push_str("| Algorithm | Tests Passed | Tests Failed | Status | Notes |\n");
    report.push_str("|-----------|--------------|--------------|--------|-------|\n");

    let mut algo_health: Vec<_> = algorithms
        .iter()
        .map(|algo| {
            let algo_metrics: Vec<_> = metrics.iter().filter(|m| m.algorithm == *algo).collect();
            let passed = algo_metrics
                .iter()
                .filter(|m| m.verification_passed)
                .count();
            let failed = algo_metrics.len() - passed;
            let all_pass = failed == 0;
            (algo, passed, failed, all_pass)
        })
        .collect();
    algo_health.sort_by(|a, b| b.3.cmp(&a.3).then(b.1.cmp(&a.1)));

    for (algo, passed, failed, all_pass) in &algo_health {
        let status = if *all_pass {
            "✅ VERIFIED"
        } else {
            "❌ FAILED"
        };
        let notes = if *all_pass {
            "All tests passed".to_string()
        } else {
            "Produces corrupted output - DO NOT USE IN PRODUCTION".to_string()
        };
        report.push_str(format!(
            "| {algo} | {passed} | {failed} | {status} | {notes} |\n"
        ).as_str());
    }
    report.push('\n');

    // Filter verified algorithms for rankings
    let verified_algos: Vec<String> = algo_health
        .iter()
        .filter(|(_, _, failed, _)| *failed == 0)
        .map(|(algo, _, _, _)| (*algo).clone())
        .collect();

    // Overall Rankings (ONLY VERIFIED)
    report.push_str("## 🏆 Overall Rankings\n\n");
    report.push_str("*Only verified algorithms included*\n\n");

    report.push_str("### By Compression Ratio (Lower is Better)\n\n");
    let mut algo_compression: Vec<_> = verified_algos
        .iter()
        .map(|algo| {
            let algo_metrics: Vec<_> = metrics
                .iter()
                .filter(|m| m.algorithm == *algo && m.verification_passed)
                .collect();
            let avg = algo_metrics
                .iter()
                .map(|m| m.compression_ratio)
                .sum::<f64>()
                / algo_metrics.len() as f64;
            (algo, avg)
        })
        .collect();
    algo_compression.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

    report.push_str("| Rank | Algorithm | Avg Ratio | Interpretation |\n");
    report.push_str("|------|-----------|-----------|----------------|\n");
    for (i, (algo, ratio)) in algo_compression.iter().enumerate() {
        let savings = (1.0 - ratio) * 100.0;
        report.push_str(format!(
            "| {} | {} | {:.3} | {:.1}% space saved |\n",
            i + 1,
            algo,
            ratio,
            savings
        ).as_str());
    }
    report.push('\n');

    report.push_str("### By Encode Speed (Lower is Better)\n\n");
    let mut algo_encode: Vec<_> = verified_algos
        .iter()
        .map(|algo| {
            let algo_metrics: Vec<_> = metrics
                .iter()
                .filter(|m| m.algorithm == *algo && m.verification_passed)
                .collect();
            let avg = algo_metrics.iter().map(|m| m.encode_time_ns).sum::<u128>()
                / algo_metrics.len() as u128;
            (algo, avg)
        })
        .collect();
    algo_encode.sort_by_key(|a| a.1);

    report.push_str("| Rank | Algorithm | Avg Encode Time | Throughput |\n");
    report.push_str("|------|-----------|-----------------|------------|\n");
    for (i, (algo, time_ns)) in algo_encode.iter().enumerate() {
        let ms = *time_ns as f64 / 1_000_000.0;
        let algo_metrics: Vec<_> = metrics
            .iter()
            .filter(|m| m.algorithm == **algo && m.verification_passed)
            .collect();
        let avg_size =
            algo_metrics.iter().map(|m| m.new_size as f64).sum::<f64>() / algo_metrics.len() as f64;
        let throughput = (avg_size / 1_000_000.0) / (ms / 1000.0);
        report.push_str(format!(
            "| {} | {} | {:.3}ms | {:.1} MB/s |\n",
            i + 1,
            algo,
            ms,
            throughput
        ).as_str());
    }
    report.push('\n');

    report.push_str("### By Decode Speed (Lower is Better)\n\n");
    let mut algo_decode: Vec<_> = verified_algos
        .iter()
        .map(|algo| {
            let algo_metrics: Vec<_> = metrics
                .iter()
                .filter(|m| m.algorithm == *algo && m.verification_passed)
                .collect();
            let avg = algo_metrics.iter().map(|m| m.decode_time_ns).sum::<u128>()
                / algo_metrics.len() as u128;
            (algo, avg)
        })
        .collect();
    algo_decode.sort_by_key(|a| a.1);

    report.push_str("| Rank | Algorithm | Avg Decode Time | Throughput |\n");
    report.push_str("|------|-----------|-----------------|------------|\n");
    for (i, (algo, time_ns)) in algo_decode.iter().enumerate() {
        let ms = *time_ns as f64 / 1_000_000.0;
        let algo_metrics: Vec<_> = metrics
            .iter()
            .filter(|m| m.algorithm == **algo && m.verification_passed)
            .collect();
        let avg_size =
            algo_metrics.iter().map(|m| m.new_size as f64).sum::<f64>() / algo_metrics.len() as f64;
        let throughput = (avg_size / 1_000_000.0) / (ms / 1000.0);
        report.push_str(format!(
            "| {} | {} | {:.3}ms | {:.1} MB/s |\n",
            i + 1,
            algo,
            ms,
            throughput
        ).as_str());
    }
    report.push('\n');

    // SCALING ANALYSIS
    report.push_str("## 📈 Performance Scaling by Size\n\n");

    let sizes: Vec<String> = metrics
        .iter()
        .map(|m| m.cache_level.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let size_order = ["cache_friendly", "memory", "large"];
    let ordered_sizes: Vec<&String> = size_order
        .iter()
        .filter_map(|s| sizes.iter().find(|size| size.as_str() == *s))
        .collect();

    report.push_str("### Compression Ratio Scaling\n\n");
    report.push_str("| Algorithm |");
    for size in &ordered_sizes {
        let typical_size = match size.as_str() {
            "cache_friendly" => " 16KB",
            "memory" => " 256KB",
            "large" => " 2MB",
            _ => "",
        };
        report.push_str(format!(" {size}{typical_size} |").as_str());
    }
    report.push_str(" Trend |\n|-----------|");
    for _ in &ordered_sizes {
        report.push_str("----------|");
    }
    report.push_str("-------|\n");

    for algo in &verified_algos {
        report.push_str(format!("| {algo} |").as_str());
        let mut ratios = Vec::new();
        for size in &ordered_sizes {
            let size_metrics: Vec<_> = metrics
                .iter()
                .filter(|m| {
                    m.algorithm == *algo && m.cache_level == **size && m.verification_passed
                })
                .collect();

            if size_metrics.is_empty() {
                report.push_str(" N/A |");
            } else {
                let avg_ratio = size_metrics
                    .iter()
                    .map(|m| m.compression_ratio)
                    .sum::<f64>()
                    / size_metrics.len() as f64;
                ratios.push(avg_ratio);
                report.push_str(format!(" {avg_ratio:.3} |").as_str());
            }
        }

        // Trend analysis
        if ratios.len() >= 2 {
            let first = ratios[0];
            let last = ratios[ratios.len() - 1];
            let change_pct = ((last - first) / first) * 100.0;
            let trend = if change_pct.abs() < 5.0 {
                "➡️ Stable"
            } else if change_pct > 0.0 {
                "⬆️ Worse with size"
            } else {
                "⬇️ Better with size"
            };
            report.push_str(format!(" {trend} ({change_pct:+.1}%) |").as_str());
        } else {
            report.push_str(" - |");
        }
        report.push('\n');
    }
    report.push('\n');

    report.push_str("### Encode Speed Scaling\n\n");
    report.push_str("| Algorithm |");
    for size in &ordered_sizes {
        let typical_size = match size.as_str() {
            "cache_friendly" => " 16KB",
            "memory" => " 256KB",
            "large" => " 2MB",
            _ => "",
        };
        report.push_str(format!(" {size}{typical_size} |").as_str());
    }
    report.push_str(" Throughput Trend |\n|-----------|");
    for _ in &ordered_sizes {
        report.push_str("----------|");
    }
    report.push_str("------------------|\n");

    for algo in &verified_algos {
        report.push_str(format!("| {algo} |").as_str());
        let mut throughputs = Vec::new();
        for size in &ordered_sizes {
            let size_metrics: Vec<_> = metrics
                .iter()
                .filter(|m| {
                    m.algorithm == *algo && m.cache_level == **size && m.verification_passed
                })
                .collect();

            if size_metrics.is_empty() {
                report.push_str(" N/A |");
            } else {
                let avg_time = size_metrics
                    .iter()
                    .map(|m| m.encode_time_ns as f64 / 1_000.0)
                    .sum::<f64>()
                    / size_metrics.len() as f64;
                let avg_size =
                    size_metrics.iter().map(|m| m.new_size).sum::<usize>() / size_metrics.len();
                let throughput = (avg_size as f64 / 1_000_000.0) / (avg_time / 1_000_000.0);
                throughputs.push(throughput);

                if avg_time < 1000.0 {
                    report.push_str(format!(" {avg_time:.0}µs |").as_str());
                } else {
                    report.push_str(format!(" {:.2}ms |", avg_time / 1000.0).as_str());
                }
            }
        }

        // Throughput trend
        if throughputs.len() >= 2 {
            let first = throughputs[0];
            let last = throughputs[throughputs.len() - 1];
            let change_pct = ((last - first) / first) * 100.0;
            let trend = if change_pct.abs() < 10.0 {
                "➡️ Linear scaling"
            } else if change_pct < 0.0 {
                "⬇️ Slows with size"
            } else {
                "⬆️ Improves with size"
            };
            report.push_str(format!(" {trend} |").as_str());
        } else {
            report.push_str(" - |");
        }
        report.push('\n');
    }
    report.push('\n');

    // ACTUAL DELTA SIZES
    report.push_str("## 💾 Actual Delta Sizes\n\n");

    // Find largest size category
    let largest_size = ordered_sizes.last();
    if let Some(largest) = largest_size {
        let largest_metrics: Vec<_> = metrics
            .iter()
            .filter(|m| m.cache_level == **largest && m.verification_passed)
            .collect();

        if !largest_metrics.is_empty() {
            let typical_original = largest_metrics[0].new_size;
            report.push_str(format!(
                "For a {} file with edits:\n\n",
                format_bytes(typical_original)
            ).as_str());
            report.push_str(
                "| Algorithm | Delta Size | Original Size | Absolute Saving | Relative to Best |\n",
            );
            report.push_str(
                "|-----------|------------|---------------|-----------------|------------------|\n",
            );

            let mut size_comparison: Vec<_> = verified_algos
                .iter()
                .filter_map(|algo| {
                    let algo_metrics: Vec<_> = largest_metrics
                        .iter()
                        .filter(|m| m.algorithm == *algo)
                        .collect();

                    if algo_metrics.is_empty() {
                        return None;
                    }

                    let avg_delta = algo_metrics.iter().map(|m| m.delta_size).sum::<usize>()
                        / algo_metrics.len();
                    let avg_original =
                        algo_metrics.iter().map(|m| m.new_size).sum::<usize>() / algo_metrics.len();

                    Some((algo, avg_delta, avg_original))
                })
                .collect();

            size_comparison.sort_by_key(|(_, delta, _)| *delta);

            let best_delta = size_comparison.first().map_or(0, |(_, d, _)| *d);

            for (algo, delta_size, original_size) in size_comparison {
                let saving = original_size - delta_size;
                let saving_pct = (saving as f64 / original_size as f64) * 100.0;
                let relative = match delta_size.cmp(&best_delta) {
                    CmpOrdering::Greater => format!("+{}", format_bytes(delta_size - best_delta)),
                    CmpOrdering::Less => format!("-{}", format_bytes(best_delta - delta_size)),
                    CmpOrdering::Equal => "Best".to_string(),
                };

                report.push_str(format!(
                    "| {} | {} | {} | {} ({:.1}%) | {} |\n",
                    algo,
                    format_bytes(delta_size),
                    format_bytes(original_size),
                    format_bytes(saving),
                    saving_pct,
                    relative
                ).as_str());
            }
            report.push('\n');
        }
    }

    // CONSISTENCY SCORE
    report.push_str("## 🎯 Compression Consistency\n\n");
    report.push_str("How predictable is each algorithm's compression ratio?\n\n");
    report.push_str("| Algorithm | Std Dev | Coefficient of Variation | Consistency Rating |\n");
    report.push_str("|-----------|---------|--------------------------|--------------------|\n");

    let mut consistency_scores: Vec<_> = verified_algos
        .iter()
        .map(|algo| {
            let algo_metrics: Vec<_> = metrics
                .iter()
                .filter(|m| m.algorithm == *algo && m.verification_passed)
                .collect();

            let ratios: Vec<f64> = algo_metrics.iter().map(|m| m.compression_ratio).collect();
            let mean = ratios.iter().sum::<f64>() / ratios.len() as f64;
            let variance =
                ratios.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / ratios.len() as f64;
            let std_dev = variance.sqrt();
            let cv = (std_dev / mean) * 100.0;

            let rating = if cv < 5.0 {
                "⭐⭐⭐⭐⭐ Very Consistent"
            } else if cv < 10.0 {
                "⭐⭐⭐⭐ Consistent"
            } else if cv < 15.0 {
                "⭐⭐⭐ Moderate"
            } else if cv < 25.0 {
                "⭐⭐ Variable"
            } else {
                "⭐ Highly Variable"
            };

            (algo, std_dev, cv, rating)
        })
        .collect();

    consistency_scores.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap());

    for (algo, std_dev, cv, rating) in consistency_scores {
        report.push_str(format!(
            "| {algo} | {std_dev:.4} | {cv:.1}% | {rating} |\n"
        ).as_str());
    }
    report.push('\n');

    // Performance by Data Format
    let formats: Vec<String> = metrics
        .iter()
        .map(|m| m.data_format.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    report.push_str("## 📁 Performance by Data Format\n\n");

    for format in &formats {
        let format_metrics: Vec<_> = metrics
            .iter()
            .filter(|m| m.data_format == *format && m.verification_passed)
            .collect();

        if format_metrics.is_empty() {
            continue;
        }

        report.push_str(format!(
            "### {} ({} tests)\n\n",
            format,
            format_metrics.len()
        ).as_str());

        let mut format_rankings: Vec<_> = verified_algos
            .iter()
            .filter_map(|algo| {
                let algo_format_metrics: Vec<_> = format_metrics
                    .iter()
                    .filter(|m| m.algorithm == *algo)
                    .collect();

                if algo_format_metrics.is_empty() {
                    return None;
                }

                let avg_ratio = algo_format_metrics
                    .iter()
                    .map(|m| m.compression_ratio)
                    .sum::<f64>()
                    / algo_format_metrics.len() as f64;
                let avg_encode = algo_format_metrics
                    .iter()
                    .map(|m| m.encode_time_ns as f64 / 1_000_000.0)
                    .sum::<f64>()
                    / algo_format_metrics.len() as f64;
                let avg_decode = algo_format_metrics
                    .iter()
                    .map(|m| m.decode_time_ns as f64 / 1_000_000.0)
                    .sum::<f64>()
                    / algo_format_metrics.len() as f64;

                Some((algo.as_str(), avg_ratio, avg_encode, avg_decode))
            })
            .collect();

        format_rankings.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

        report.push_str("| Rank | Algorithm | Ratio | Encode (ms) | Decode (ms) | Score |\n");
        report.push_str("|------|-----------|-------|-------------|-------------|-------|\n");

        for (i, (algo, ratio, encode, decode)) in format_rankings.iter().enumerate() {
            let score = ratio * 0.6 + (encode / 1000.0) * 0.3 + (decode / 1000.0) * 0.1;
            report.push_str(format!(
                "| {} | {} | {:.3} | {:.3} | {:.3} | {:.4} |\n",
                i + 1,
                algo,
                ratio,
                encode,
                decode,
                score
            ).as_str());
        }
        report.push('\n');
    }

    // Performance by Change Pattern
    let changes: Vec<String> = metrics
        .iter()
        .map(|m| m.change_pattern.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    report.push_str("## 🔄 Performance by Change Pattern\n\n");

    for change in &changes {
        let change_metrics: Vec<_> = metrics
            .iter()
            .filter(|m| m.change_pattern == *change && m.verification_passed)
            .collect();

        if change_metrics.is_empty() {
            continue;
        }

        report.push_str(format!(
            "### {} ({} tests)\n\n",
            change,
            change_metrics.len()
        ).as_str());

        let mut change_rankings: Vec<_> = verified_algos
            .iter()
            .filter_map(|algo| {
                let algo_change_metrics: Vec<_> = change_metrics
                    .iter()
                    .filter(|m| m.algorithm == *algo)
                    .collect();

                if algo_change_metrics.is_empty() {
                    return None;
                }

                let avg_ratio = algo_change_metrics
                    .iter()
                    .map(|m| m.compression_ratio)
                    .sum::<f64>()
                    / algo_change_metrics.len() as f64;
                let avg_encode = algo_change_metrics
                    .iter()
                    .map(|m| m.encode_time_ns as f64 / 1_000_000.0)
                    .sum::<f64>()
                    / algo_change_metrics.len() as f64;

                Some((algo.as_str(), avg_ratio, avg_encode))
            })
            .collect();

        change_rankings.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

        report.push_str("| Rank | Algorithm | Compression | Encode Time | Efficiency Score |\n");
        report.push_str("|------|-----------|-------------|-------------|------------------|\n");

        for (i, (algo, ratio, encode)) in change_rankings.iter().enumerate() {
            let efficiency = if *encode > 0.0 {
                (1.0 - *ratio) / (*encode / 1000.0)
            } else {
                0.0
            };
            report.push_str(format!(
                "| {} | {} | {:.3} | {:.3}ms | {:.4} |\n",
                i + 1,
                algo,
                ratio,
                encode,
                efficiency
            ).as_str());
        }
        report.push('\n');
    }

    // Algorithm Deep Dive
    report.push_str("## 🔍 Algorithm Deep Dive\n\n");

    for algo in &verified_algos {
        let algo_metrics: Vec<_> = metrics
            .iter()
            .filter(|m| m.algorithm == *algo && m.verification_passed)
            .collect();

        report.push_str(format!("### {algo}\n\n").as_str());
        report.push_str(format!("**Total Tests:** {}\n\n", algo_metrics.len()).as_str());

        let ratios: Vec<f64> = algo_metrics.iter().map(|m| m.compression_ratio).collect();
        let avg_ratio = ratios.iter().sum::<f64>() / ratios.len() as f64;
        let mut sorted_ratios = ratios.clone();
        sorted_ratios.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median_ratio = sorted_ratios[sorted_ratios.len() / 2];
        let best_ratio = sorted_ratios[0];
        let worst_ratio = sorted_ratios[sorted_ratios.len() - 1];

        report.push_str("**Compression Statistics:**\n\n");
        report.push_str("| Metric | Value | Space Saved |\n");
        report.push_str("|--------|-------|-------------|\n");
        report.push_str(format!(
            "| Average | {:.3} | {:.1}% |\n",
            avg_ratio,
            (1.0 - avg_ratio) * 100.0
        ).as_str());
        report.push_str(format!(
            "| Median | {:.3} | {:.1}% |\n",
            median_ratio,
            (1.0 - median_ratio) * 100.0
        ).as_str());
        report.push_str(format!(
            "| Best | {:.3} | {:.1}% |\n",
            best_ratio,
            (1.0 - best_ratio) * 100.0
        ).as_str());
        report.push_str(format!(
            "| Worst | {:.3} | {:.1}% |\n\n",
            worst_ratio,
            (1.0 - worst_ratio) * 100.0
        ).as_str());

        let best_test = algo_metrics
            .iter()
            .min_by(|a, b| {
                a.compression_ratio
                    .partial_cmp(&b.compression_ratio)
                    .unwrap()
            })
            .unwrap();
        let worst_test = algo_metrics
            .iter()
            .max_by(|a, b| {
                a.compression_ratio
                    .partial_cmp(&b.compression_ratio)
                    .unwrap()
            })
            .unwrap();

        report.push_str("**Performance Highlights:**\n\n");
        report.push_str(format!(
            "- Best on: {} / {} / {} ({:.3} ratio)\n",
            best_test.data_format,
            best_test.change_pattern,
            best_test.cache_level,
            best_test.compression_ratio
        ).as_str());
        report.push_str(format!(
            "- Worst on: {} / {} / {} ({:.3} ratio)\n\n",
            worst_test.data_format,
            worst_test.change_pattern,
            worst_test.cache_level,
            worst_test.compression_ratio
        ).as_str());
    }

    // Head-to-Head Comparison
    report.push_str("## ⚔️ Head-to-Head Comparison\n\n");
    report.push_str("### Win Matrix (Compression Ratio)\n\n");
    report.push_str("Rows beat Columns (% of direct matchups won)\n\n");

    report.push_str("|  |");
    for algo in &verified_algos {
        report.push_str(format!(" {algo} |").as_str());
    }
    report.push_str("\n|");
    report.push_str("--|");
    for _ in &verified_algos {
        report.push_str("-----|");
    }
    report.push('\n');

    for algo1 in &verified_algos {
        report.push_str(format!("| {algo1} |").as_str());
        for algo2 in &verified_algos {
            if algo1 == algo2 {
                report.push_str(" - |");
                continue;
            }

            let mut wins = 0;
            let mut total = 0;

            for format in &formats {
                for change in &changes {
                    for size in &sizes {
                        let m1: Vec<_> = metrics
                            .iter()
                            .filter(|m| {
                                m.algorithm == *algo1
                                    && m.data_format == *format
                                    && m.change_pattern == *change
                                    && m.cache_level == *size
                                    && m.verification_passed
                            })
                            .collect();

                        let m2: Vec<_> = metrics
                            .iter()
                            .filter(|m| {
                                m.algorithm == *algo2
                                    && m.data_format == *format
                                    && m.change_pattern == *change
                                    && m.cache_level == *size
                                    && m.verification_passed
                            })
                            .collect();

                        if !m1.is_empty() && !m2.is_empty() {
                            total += 1;
                            if m1[0].compression_ratio < m2[0].compression_ratio {
                                wins += 1;
                            }
                        }
                    }
                }
            }

            let win_rate = if total > 0 {
                (f64::from(wins) / f64::from(total)) * 100.0
            } else {
                0.0
            };
            report.push_str(format!(" {win_rate:.0}% |").as_str());
        }
        report.push('\n');
    }
    report.push('\n');

    // Speed vs Compression Trade-offs
    report.push_str("## ⚖️ Speed vs Compression Trade-offs\n\n");
    report.push_str("| Algorithm | Avg Ratio | Avg Encode (ms) | Efficiency | Category |\n");
    report.push_str("|-----------|-----------|-----------------|------------|----------|\n");

    let mut tradeoffs: Vec<_> = verified_algos
        .iter()
        .map(|algo| {
            let algo_metrics: Vec<_> = metrics
                .iter()
                .filter(|m| m.algorithm == *algo && m.verification_passed)
                .collect();
            let avg_ratio = algo_metrics
                .iter()
                .map(|m| m.compression_ratio)
                .sum::<f64>()
                / algo_metrics.len() as f64;
            let avg_encode = algo_metrics
                .iter()
                .map(|m| m.encode_time_ns as f64 / 1_000_000.0)
                .sum::<f64>()
                / algo_metrics.len() as f64;
            let efficiency = (1.0 - avg_ratio) / (avg_encode / 1000.0);

            let category = if avg_ratio < 0.15 && avg_encode < 5.0 {
                "🏆 Best Overall"
            } else if avg_ratio < 0.15 {
                "🎯 Best Compression"
            } else if avg_encode < 3.0 {
                "⚡ Fastest"
            } else {
                "⚖️ Balanced"
            };

            (algo, avg_ratio, avg_encode, efficiency, category)
        })
        .collect();

    tradeoffs.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap());

    for (algo, ratio, encode, efficiency, category) in tradeoffs {
        report.push_str(format!(
            "| {algo} | {ratio:.3} | {encode:.3} | {efficiency:.4} | {category} |\n"
        ).as_str());
    }
    report.push('\n');

    // ROI ANALYSIS
    report.push_str("## 💰 Compression ROI Analysis\n\n");
    report.push_str("Is better compression worth slower encode speed?\n\n");
    report.push_str("| Comparison | Time Difference | Compression Difference | Bytes Saved per ms | Worth It? |\n");
    report.push_str("|------------|-----------------|------------------------|-------------------|----------|\n");

    for i in 0..verified_algos.len() {
        for j in (i + 1)..verified_algos.len() {
            let algo1 = &verified_algos[i];
            let algo2 = &verified_algos[j];

            let m1: Vec<_> = metrics
                .iter()
                .filter(|m| m.algorithm == *algo1 && m.verification_passed)
                .collect();
            let m2: Vec<_> = metrics
                .iter()
                .filter(|m| m.algorithm == *algo2 && m.verification_passed)
                .collect();

            if m1.is_empty() || m2.is_empty() {
                continue;
            }

            let avg_time1 = m1
                .iter()
                .map(|m| m.encode_time_ns as f64 / 1_000_000.0)
                .sum::<f64>()
                / m1.len() as f64;
            let avg_time2 = m2
                .iter()
                .map(|m| m.encode_time_ns as f64 / 1_000_000.0)
                .sum::<f64>()
                / m2.len() as f64;
            let avg_ratio1 = m1.iter().map(|m| m.compression_ratio).sum::<f64>() / m1.len() as f64;
            let avg_ratio2 = m2.iter().map(|m| m.compression_ratio).sum::<f64>() / m2.len() as f64;
            let avg_size = m1.iter().map(|m| m.new_size as f64).sum::<f64>() / m1.len() as f64;

            let time_diff = (avg_time2 - avg_time1).abs();
            let ratio_diff = (avg_ratio2 - avg_ratio1).abs();
            let bytes_saved = (ratio_diff * avg_size) / time_diff;

            let faster = if avg_time1 < avg_time2 { algo1 } else { algo2 };
            let better_compression = if avg_ratio1 < avg_ratio2 {
                algo1
            } else {
                algo2
            };

            let worth_it = if bytes_saved > 100_000.0 {
                "✅ Yes"
            } else if bytes_saved > 10_000.0 {
                "🤔 Maybe"
            } else {
                "❌ Minimal gain"
            };

            report.push_str(format!(
                "| {} → {} | {:+.3}ms | {:.1}% | {:.0} KB/ms | {} |\n",
                faster,
                better_compression,
                time_diff,
                ratio_diff * 100.0,
                bytes_saved / 1000.0,
                worth_it
            ).as_str());
        }
    }
    report.push('\n');

    // QUICK DECISION MATRIX
    report.push_str("## 🎯 Quick Decision Matrix\n\n");
    report.push_str("| Your Priority | Recommended | Why | Alternative |\n");
    report.push_str("|---------------|-------------|-----|-------------|\n");

    // Max compression
    let best_compression = algo_compression.first();
    if let Some((algo, ratio)) = best_compression {
        let runner_up = algo_compression.get(1);
        report.push_str(format!(
            "| Maximum Compression | {} | {:.1}% space saved | {} |\n",
            algo,
            (1.0 - ratio) * 100.0,
            runner_up.map_or("N/A", |(a, _)| a.as_str())
        ).as_str());
    }

    // Max speed
    let fastest = algo_encode.first();
    if let Some((algo, time_ns)) = fastest {
        let runner_up = algo_encode.get(1);
        report.push_str(format!(
            "| Maximum Speed | {} | {:.1} MB/s encode | {} |\n",
            algo,
            {
                let algo_metrics: Vec<_> = metrics
                    .iter()
                    .filter(|m| m.algorithm == **algo && m.verification_passed)
                    .collect();
                let avg_size = algo_metrics.iter().map(|m| m.new_size as f64).sum::<f64>()
                    / algo_metrics.len() as f64;
                (avg_size / 1_000_000.0) / ((*time_ns as f64 / 1_000_000.0) / 1000.0)
            },
            runner_up.map_or("N/A", |(a, _)| a.as_str())
        ).as_str());
    }

    // Balanced
    let balanced_idx = verified_algos.len() / 2;
    if balanced_idx < verified_algos.len() {
        let balanced = &verified_algos[balanced_idx];
        report.push_str(format!(
            "| Balanced | {} | Good mix of speed and compression | {} |\n",
            balanced,
            verified_algos
                .get(balanced_idx + 1)
                .unwrap_or(&verified_algos[0])
        ).as_str());
    }

    // Real-time
    let fastest_decode = algo_decode.first();
    if let Some((algo, _)) = fastest_decode {
        report.push_str(format!(
            "| Real-time Decode | {} | Fastest reconstruction | {} |\n",
            algo,
            algo_decode.get(1).map_or("N/A", |(a, _)| a.as_str())
        ).as_str());
    }

    report.push('\n');

    // PATTERN-SPECIFIC RECOMMENDATIONS
    report.push_str("## 🔄 Pattern-Specific Recommendations\n\n");
    report.push_str("| Change Pattern | Best Algorithm | Runner-up | Key Metric |\n");
    report.push_str("|----------------|----------------|-----------|------------|\n");

    for change in &changes {
        let change_metrics: Vec<_> = metrics
            .iter()
            .filter(|m| m.change_pattern == *change && m.verification_passed)
            .collect();

        if change_metrics.is_empty() {
            report.push_str(format!(
                "| {change} | *No data* | - | Run more tests |\n"
            ).as_str());
            continue;
        }

        let mut pattern_rankings: Vec<_> = verified_algos
            .iter()
            .filter_map(|algo| {
                let algo_metrics: Vec<_> = change_metrics
                    .iter()
                    .filter(|m| m.algorithm == *algo)
                    .collect();

                if algo_metrics.is_empty() {
                    return None;
                }

                let avg_ratio = algo_metrics
                    .iter()
                    .map(|m| m.compression_ratio)
                    .sum::<f64>()
                    / algo_metrics.len() as f64;
                Some((algo, avg_ratio))
            })
            .collect();

        pattern_rankings.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

        let best = pattern_rankings.first();
        let runner_up = pattern_rankings.get(1);

        if let Some((algo, ratio)) = best {
            report.push_str(format!(
                "| {} | {} | {} | {:.1}% compression |\n",
                change,
                algo,
                runner_up.map_or("-", |(a, _)| a.as_str()),
                (1.0 - ratio) * 100.0
            ).as_str());
        }
    }
    report.push('\n');

    // WHAT NOT TO USE
    report.push_str("## 🚫 What NOT to Use\n\n");

    let failed_algos: Vec<_> = algo_health
        .iter()
        .filter(|(_, _, failed, _)| *failed > 0)
        .collect();

    if !failed_algos.is_empty() {
        report.push_str("### ❌ Failed Verification\n\n");
        report.push_str("| Algorithm | Reason | Status |\n");
        report.push_str("|-----------|--------|--------|\n");

        for (algo, _, failed, _) in failed_algos {
            report.push_str(format!(
                "| {} | Failed {} out of {} tests - produces corrupted output | ⛔ DO NOT USE |\n",
                algo,
                failed,
                metrics.iter().filter(|m| m.algorithm == **algo).count()
            ).as_str());
        }
        report.push('\n');
    }

    report.push_str("### 💡 Additional Guidance\n\n");
    report.push_str("- **For production use:** Only use algorithms with ✅ VERIFIED status\n");
    report.push_str("- **For critical data:** Always verify reconstruction matches original\n");
    report.push_str("- **For large files:** Run full benchmark with `BENCH_MODE=full`\n");
    report.push_str("- **For specific use cases:** Test with your actual data patterns\n\n");

    // Footer
    report.push_str("---\n\n");
    report.push_str("*Generated by gdelta comprehensive benchmark suite*\n");
    report.push_str("\n**Run more tests with:**\n");
    report.push_str("````bash\n");
    report.push_str("# Test all formats\n");
    report.push_str("cargo bench --bench comprehensive\n\n");
    report.push_str("# Test specific scenarios\n");
    report.push_str("BENCH_FORMATS=csv,logs BENCH_PATTERNS=append_1024 cargo bench\n\n");
    report.push_str("# Full benchmark (takes longer)\n");
    report.push_str("BENCH_MODE=full cargo bench\n");
    report.push_str("````\n");

    std::fs::write(output_path, report)?;
    println!(
        "\n✅ Comprehensive markdown report generated: {output_path}"
    );

    Ok(())
}

// Helper function for formatting bytes
#[allow(clippy::cast_precision_loss)]
fn format_bytes(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn generate_json_report(
    metrics: Vec<BenchmarkMetric>,
    hardware: HardwareInfo,
    early_termination: bool,
    output_path: &str,
) -> std::io::Result<()> {
    let report = BenchmarkReport {
        generated_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        hardware,
        metrics,
        early_termination,
    };

    let json = serde_json::to_string_pretty(&report)?;
    std::fs::write(output_path, json)?;
    println!("✅ JSON report generated: {output_path}");

    Ok(())
}

// ============================================================================
// Benchmark Configuration
// ============================================================================

#[derive(Clone)]
struct BenchmarkConfig {
    sample_size: usize,
    measurement_time_secs: u64,
    warmup_time_millis: u64,

    // Filters - None means "run all"
    algorithms: Option<Vec<String>>,
    formats: Option<Vec<String>>,
    change_patterns: Option<Vec<String>>,
    sizes: Option<Vec<String>>,
}

impl BenchmarkConfig {
    fn quick() -> Self {
        Self {
            sample_size: 10,
            measurement_time_secs: 1,
            warmup_time_millis: 500,
            algorithms: None,
            formats: None,
            change_patterns: None,
            sizes: None,
        }
    }

    fn full() -> Self {
        Self {
            sample_size: 100,
            measurement_time_secs: 5,
            warmup_time_millis: 1000,
            algorithms: None,
            formats: None,
            change_patterns: None,
            sizes: None,
        }
    }

    fn from_env() -> Self {
        let mode = std::env::var("BENCH_MODE").unwrap_or_else(|_| "quick".to_string());

        let mut config = match mode.as_str() {
            "full" => Self::full(),
            _ => Self::quick(),
        };

        // Parse filters from environment
        if let Ok(algos) = std::env::var("BENCH_ALGOS") {
            config.algorithms = Some(algos.split(',').map(|s| s.trim().to_string()).collect());
        }

        if let Ok(formats) = std::env::var("BENCH_FORMATS") {
            config.formats = Some(formats.split(',').map(|s| s.trim().to_string()).collect());
        }

        if let Ok(patterns) = std::env::var("BENCH_PATTERNS") {
            config.change_patterns =
                Some(patterns.split(',').map(|s| s.trim().to_string()).collect());
        }

        if let Ok(sizes) = std::env::var("BENCH_SIZES") {
            config.sizes = Some(sizes.split(',').map(|s| s.trim().to_string()).collect());
        }

        config
    }

    fn should_run_algorithm(&self, name: &str) -> bool {
        self.algorithms
            .as_ref()
            .is_none_or(|list| list.contains(&name.to_string()))
    }

    fn should_run_format(&self, name: &str) -> bool {
        self.formats
            .as_ref()
            .is_none_or(|list| list.contains(&name.to_string()))
    }

    fn should_run_pattern(&self, name: &str) -> bool {
        self.change_patterns
            .as_ref()
            .is_none_or(|list| list.contains(&name.to_string()))
    }

    fn should_run_size(&self, name: &str) -> bool {
        self.sizes
            .as_ref()
            .is_none_or(|list| list.contains(&name.to_string()))
    }

    fn print_info(&self) {
        println!("📋 Benchmark Configuration:");
        println!(
            "   Mode: {} samples, {}s measurement",
            self.sample_size, self.measurement_time_secs
        );

        if let Some(algos) = &self.algorithms {
            println!("   Algorithms: {}", algos.join(", "));
        } else {
            println!("   Algorithms: all");
        }

        if let Some(formats) = &self.formats {
            println!("   Formats: {}", formats.join(", "));
        } else {
            println!("   Formats: all");
        }

        if let Some(patterns) = &self.change_patterns {
            println!("   Patterns: {}", patterns.join(", "));
        } else {
            println!("   Patterns: all");
        }

        if let Some(sizes) = &self.sizes {
            println!("   Sizes: {}", sizes.join(", "));
        } else {
            println!("   Sizes: all");
        }
        println!();
    }
}

// ============================================================================
// Criterion Benchmarks
// ============================================================================

#[allow(clippy::too_many_lines)]
fn run_benchmarks_with_config(c: &mut Criterion, config: &BenchmarkConfig) {
    setup_signal_handler();

    let timestamp = get_timestamp();
    let wal_file = get_wal_file(timestamp.as_str());
    let report_md = get_report_md(timestamp.as_str());
    let report_json = get_report_json(timestamp.as_str());

    println!("📁 Results will be saved with timestamp: {timestamp}");

    let wal = MetricsWal::new(wal_file.as_str()).unwrap();
    let hardware = collect_hardware_info();

    println!("\n🚀 Starting comprehensive delta compression benchmarks...\n");
    println!("💡 Press Ctrl+C to stop early and generate report with collected data\n");
    config.print_info();

    let all_algos: Vec<Box<dyn DeltaAlgorithm>> = vec![
        Box::new(GdeltaAlgorithm),
        Box::new(GdeltaZstdAlgorithm),
        Box::new(GdeltaLz4Algorithm),
        Box::new(XpatchAlgorithm),
        Box::new(Xdelta3Algorithm),
        Box::new(QbsdiffAlgorithm),
        Box::new(ZstdDictAlgorithm),
    ];

    let all_formats = vec![
        DataFormat::Json,
        DataFormat::Xml,
        DataFormat::Csv,
        DataFormat::Logs,
        DataFormat::SourceCode,
        DataFormat::Markdown,
        DataFormat::SqlDump,
        DataFormat::Protobuf,
        DataFormat::Compressed,
        DataFormat::ImageData,
        DataFormat::DatabasePage,
        DataFormat::Email,
        DataFormat::Html,
        DataFormat::Yaml,
        DataFormat::PlainText,
    ];

    let all_changes = vec![
        ChangePattern::MinorEdit,
        ChangePattern::ModerateEdit,
        ChangePattern::MajorRewrite,
        ChangePattern::Append(1024),
        ChangePattern::Insert {
            position_pct: 0.5,
            size: 512,
        },
        ChangePattern::Delete {
            position_pct: 0.3,
            size: 256,
        },
        ChangePattern::LineChanges { pct: 0.1 },
    ];

    let all_sizes = vec![
        ("cache_friendly", 16 * 1024),
        ("memory", 256 * 1024),
        ("large", 2 * 1024 * 1024),
    ];

    // Filter based on config
    let algos: Vec<_> = all_algos
        .into_iter()
        .filter(|algo| config.should_run_algorithm(algo.name()))
        .collect();

    let formats: Vec<_> = all_formats
        .into_iter()
        .filter(|format| config.should_run_format(format.name()))
        .collect();

    let changes: Vec<_> = all_changes
        .into_iter()
        .filter(|change| config.should_run_pattern(&change.name()))
        .collect();

    let sizes: Vec<_> = all_sizes
        .into_iter()
        .filter(|(name, _)| config.should_run_size(name))
        .collect();

    let total_tests = algos.len() * formats.len() * changes.len() * sizes.len();
    println!("📊 Running {total_tests} test combinations\n");

    let mut completed = 0;
    let mut early_termination = false;

    'outer: for algo in &algos {
        for format in &formats {
            for change in &changes {
                for (size_name, size) in &sizes {
                    if !should_continue() {
                        println!("\n\n🛑 Stopping benchmark early...");
                        early_termination = true;
                        break 'outer;
                    }

                    completed += 1;
                    print!("\r⏳ Progress: {completed}/{total_tests} ");
                    std::io::Write::flush(&mut std::io::stdout()).ok();

                    let base = format.generate(*size);
                    let new = change.apply(&base);

                    if let Some(metric) = run_benchmark(
                        algo.as_ref(),
                        *format,
                        *change,
                        "memory",
                        size_name,
                        &base,
                        &new,
                    ) {
                        wal.append(&metric).ok();

                        let bench_id = format!(
                            "{}_{}_{}_{}",
                            algo.name(),
                            format.name(),
                            change.name(),
                            size_name
                        );

                        let mut group = c.benchmark_group("comprehensive");
                        group.sample_size(config.sample_size);
                        group.measurement_time(std::time::Duration::from_secs(
                            config.measurement_time_secs,
                        ));
                        group.warm_up_time(std::time::Duration::from_millis(
                            config.warmup_time_millis,
                        ));
                        group.throughput(Throughput::Bytes(*size as u64));

                        group.bench_function(&bench_id, |b| {
                            b.iter(|| {
                                if let Ok(delta) = algo.encode(black_box(&new), black_box(&base)) {
                                    let _ = algo.decode(black_box(&delta), black_box(&base));
                                }
                            });
                        });

                        group.finish();
                    }
                }
            }
        }
    }

    println!("\n\n✅ Benchmark complete! generating reports...\n");

    // generate reports
    let all_metrics = wal.read_all().unwrap();
    if !all_metrics.is_empty() {
        generate_markdown_report(&all_metrics, &hardware, early_termination, &report_md).unwrap();
        generate_json_report(
            all_metrics,
            hardware.clone(),
            early_termination,
            &report_json,
        )
        .unwrap();
    }
}

fn comprehensive_benchmark(c: &mut Criterion) {
    let config = BenchmarkConfig::from_env();
    run_benchmarks_with_config(c, &config);
}

criterion_group!(benches, comprehensive_benchmark);
criterion_main!(benches);
