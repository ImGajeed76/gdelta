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
//! Quick mode: BENCH_MODE=quick cargo bench --bench comprehensive
//! Full mode: BENCH_MODE=full cargo bench --bench comprehensive
//! Custom: BENCH_ALGOS=gdelta,xpatch BENCH_FORMATS=json,csv cargo bench --bench comprehensive
//! View report: cat target/benchmark_report.md

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use fake::Fake;
use fake::faker::internet::en::*;
use fake::faker::lorem::en::*;
use fake::faker::name::en::*;
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

// ============================================================================
// Configuration
// ============================================================================

fn get_timestamp() -> String {
    chrono::Local::now().format("%Y%m%d_%H%M%S").to_string()
}

fn get_results_dir(timestamp: &str) -> String {
    format!("target/benchmark_results_{}", timestamp)
}

fn get_wal_file(timestamp: &str) -> String {
    format!("target/benchmark_results_{}/metrics.wal", timestamp)
}

fn get_report_md(timestamp: &str) -> String {
    format!("target/benchmark_report_{}.md", timestamp)
}

fn get_report_json(timestamp: &str) -> String {
    format!("target/benchmark_report_{}.json", timestamp)
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
    fn name(&self) -> &str {
        "gdelta"
    }

    fn encode(&self, new: &[u8], base: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        encode(new, base).map_err(|e| e.into())
    }

    fn decode(&self, delta: &[u8], base: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        decode(delta, base).map_err(|e| e.into())
    }
}

// Gdelta with Zstd compression
struct GdeltaZstdAlgorithm;

impl DeltaAlgorithm for GdeltaZstdAlgorithm {
    fn name(&self) -> &str {
        "gdelta_zstd"
    }

    fn encode(&self, new: &[u8], base: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let delta = encode(new, base)?;
        let compressed = zstd::encode_all(&delta[..], 3)?; // Level 3 for speed
        Ok(compressed)
    }

    fn decode(&self, delta: &[u8], base: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let decompressed = zstd::decode_all(delta)?;
        decode(&decompressed, base).map_err(|e| e.into())
    }
}

// Gdelta with LZ4 compression
struct GdeltaLz4Algorithm;

impl DeltaAlgorithm for GdeltaLz4Algorithm {
    fn name(&self) -> &str {
        "gdelta_lz4"
    }

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
        decode(&decompressed, base).map_err(|e| e.into())
    }
}

// XPatch (uses gdelta internally with automatic algorithm selection)
struct XpatchAlgorithm;

impl DeltaAlgorithm for XpatchAlgorithm {
    fn name(&self) -> &str {
        "xpatch"
    }

    fn encode(&self, new: &[u8], base: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let tag = 0; // No metadata needed for benchmarking
        let enable_zstd = true; // Enable for better compression
        Ok(xpatch::delta::encode(tag, base, new, enable_zstd))
    }

    fn decode(&self, delta: &[u8], base: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        xpatch::delta::decode(base, delta).map_err(|e| e.into())
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

    fn generate(&self, size_target: usize) -> Vec<u8> {
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

        data.push_str(&format!(
            "  {{\"id\": {}, \"name\": \"{}\", \"email\": \"{}\", \"active\": {}}},\n",
            id,
            name,
            email,
            rng.random_bool(0.8)
        ));
    }

    data.push_str("]\n");
    data.into_bytes()
}

fn generate_xml(size_target: usize, rng: &mut StdRng) -> Vec<u8> {
    let mut data = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<root>\n");

    while data.len() < size_target {
        let name: String = Name().fake_with_rng(rng);
        let content: String = Sentence(3..10).fake_with_rng(rng);

        data.push_str(&format!(
            "  <item id=\"{}\">\n    <name>{}</name>\n    <content>{}</content>\n  </item>\n",
            rng.random_range(1000..99999),
            name,
            content
        ));
    }

    data.push_str("</root>\n");
    data.into_bytes()
}

fn generate_csv(size_target: usize, rng: &mut StdRng) -> Vec<u8> {
    let mut data = String::from("id,name,email,timestamp,value\n");

    while data.len() < size_target {
        let name: String = Name().fake_with_rng(rng);
        let email: String = SafeEmail().fake_with_rng(rng);
        let timestamp = 1700000000 + rng.random_range(0..10000000);
        let value = rng.random_range(0.0..1000.0);

        data.push_str(&format!(
            "{},{},{},{},{:.2}\n",
            rng.random_range(1000..99999),
            name,
            email,
            timestamp,
            value
        ));
    }

    data.into_bytes()
}

fn generate_logs(size_target: usize, rng: &mut StdRng) -> Vec<u8> {
    let mut data = String::new();
    let levels = ["INFO", "WARN", "ERROR", "DEBUG"];

    while data.len() < size_target {
        let level = levels[rng.random_range(0..levels.len())];
        let timestamp = 1700000000 + rng.random_range(0..10000000);
        let message: String = Sentence(5..15).fake_with_rng(rng);

        data.push_str(&format!(
            "[{}] {} [thread-{}] {}\n",
            timestamp,
            level,
            rng.random_range(1..20),
            message
        ));
    }

    data.into_bytes()
}

fn generate_source_code(size_target: usize, rng: &mut StdRng) -> Vec<u8> {
    let mut data = String::from("fn main() {\n");

    while data.len() < size_target {
        let var_name = format!("var_{}", rng.random_range(0..100));
        let value = rng.random_range(0..1000);

        data.push_str(&format!("    let {} = {};\n", var_name, value));

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
        data.push_str(&format!("## Section {}\n\n", rng.random_range(1..100)));

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

        data.push_str(&format!(
            "INSERT INTO users (id, name, email) VALUES ({}, '{}', '{}');\n",
            rng.random_range(1000..99999),
            name,
            email
        ));
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
        data.extend(std::iter::repeat(0u8).take(padding));
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

        data.push_str(&format!(
            "<div class=\"item\">\n  <h2>{}</h2>\n  <p>{}</p>\n</div>\n",
            title, content
        ));
    }

    data.push_str("</body>\n</html>\n");
    data.into_bytes()
}

fn generate_yaml(size_target: usize, rng: &mut StdRng) -> Vec<u8> {
    let mut data = String::from("config:\n");

    while data.len() < size_target {
        let key = format!("setting_{}", rng.random_range(0..100));
        let value = rng.random_range(0..1000);

        data.push_str(&format!("  {}: {}\n", key, value));

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
    fn name(&self) -> String {
        match self {
            ChangePattern::MinorEdit => "minor_edit".to_string(),
            ChangePattern::ModerateEdit => "moderate_edit".to_string(),
            ChangePattern::MajorRewrite => "major_rewrite".to_string(),
            ChangePattern::Append(n) => format!("append_{}", n),
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
        writeln!(file, "{}", json)?;

        Ok(())
    }

    fn read_all(&self) -> std::io::Result<Vec<BenchmarkMetric>> {
        if !Path::new(&self.path).exists() {
            return Ok(Vec::new());
        }

        let file = File::open(&self.path)?;
        let reader = BufReader::new(file);
        let mut metrics = Vec::new();

        for line in reader.lines() {
            if let Ok(line) = line {
                if let Ok(metric) = serde_json::from_str::<BenchmarkMetric>(&line) {
                    metrics.push(metric);
                }
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
        .first()
        .map(|cpu| cpu.brand().to_string())
        .unwrap_or_else(|| "Unknown CPU".to_string());

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
    let reconstructed = match algo.decode(&delta, base) {
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

    report.push_str(&format!(
        "**generated:** {}\n\n",
        chrono::DateTime::<chrono::Utc>::from(SystemTime::now()).format("%Y-%m-%d %H:%M:%S UTC")
    ));

    // Hardware
    report.push_str("## 💻 Hardware Configuration\n\n");
    report.push_str("````\n");
    report.push_str(&format!("CPU:    {}\n", hardware.cpu_brand));
    report.push_str(&format!("Cores:  {}\n", hardware.cpu_cores));
    report.push_str(&format!("RAM:    {} MB\n", hardware.total_memory_mb));
    report.push_str(&format!("OS:     {}\n", hardware.os));
    report.push_str("````\n\n");

    // Executive Summary
    report.push_str("## 📊 Executive Summary\n\n");

    let total_tests = metrics.len();
    let passed = metrics.iter().filter(|m| m.verification_passed).count();
    let failed = total_tests - passed;

    let avg_compression =
        metrics.iter().map(|m| m.compression_ratio).sum::<f64>() / total_tests as f64;
    let median_compression = {
        let mut ratios: Vec<f64> = metrics.iter().map(|m| m.compression_ratio).collect();
        ratios.sort_by(|a, b| a.partial_cmp(b).unwrap());
        if ratios.is_empty() {
            0.0
        } else {
            ratios[ratios.len() / 2]
        }
    };

    let best_compression = metrics.iter().min_by(|a, b| {
        a.compression_ratio
            .partial_cmp(&b.compression_ratio)
            .unwrap()
    });

    let total_encode_time_ms = metrics
        .iter()
        .map(|m| m.encode_time_ns as f64 / 1_000_000.0)
        .sum::<f64>();
    let total_decode_time_ms = metrics
        .iter()
        .map(|m| m.decode_time_ns as f64 / 1_000_000.0)
        .sum::<f64>();

    report.push_str(&format!("- **Total Tests:** {}\n", total_tests));
    report.push_str(&format!(
        "- **Verification:** {} passed, {} failed ({:.1}% success rate)\n",
        passed,
        failed,
        (passed as f64 / total_tests as f64) * 100.0
    ));
    report.push_str(&format!(
        "- **Average Compression:** {:.2}% of original size\n",
        avg_compression * 100.0
    ));
    report.push_str(&format!(
        "- **Median Compression:** {:.2}%\n",
        median_compression * 100.0
    ));
    report.push_str(&format!(
        "- **Best Compression:** {:.2}% ({})\n",
        best_compression
            .map(|m| m.compression_ratio * 100.0)
            .unwrap_or(0.0),
        best_compression
            .map(|m| m.algorithm.as_str())
            .unwrap_or("N/A")
    ));
    report.push_str(&format!(
        "- **Total Encode Time:** {:.2}s\n",
        total_encode_time_ms / 1000.0
    ));
    report.push_str(&format!(
        "- **Total Decode Time:** {:.2}s\n\n",
        total_decode_time_ms / 1000.0
    ));

    // Algorithm Comparison Matrix
    report.push_str("## 🏆 Algorithm Performance Matrix\n\n");

    let algorithms: Vec<String> = metrics
        .iter()
        .map(|m| m.algorithm.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    report.push_str("| Algorithm | Tests | Avg Compression | Median Compression | Best | Avg Encode (ms) | Avg Decode (ms) | Throughput (MB/s) |\n");
    report.push_str("|-----------|-------|-----------------|--------------------|------|-----------------|-----------------|-------------------|\n");

    for algo in &algorithms {
        let algo_metrics: Vec<_> = metrics.iter().filter(|m| m.algorithm == *algo).collect();
        let count = algo_metrics.len();
        if count == 0 {
            continue;
        }

        let avg_comp = algo_metrics
            .iter()
            .map(|m| m.compression_ratio)
            .sum::<f64>()
            / count as f64;

        let mut comps: Vec<f64> = algo_metrics.iter().map(|m| m.compression_ratio).collect();
        comps.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median_comp = comps[comps.len() / 2];
        let best_comp = comps[0];

        let avg_encode = algo_metrics
            .iter()
            .map(|m| m.encode_time_ns as f64 / 1_000_000.0)
            .sum::<f64>()
            / count as f64;
        let avg_decode = algo_metrics
            .iter()
            .map(|m| m.decode_time_ns as f64 / 1_000_000.0)
            .sum::<f64>()
            / count as f64;

        let avg_size = algo_metrics.iter().map(|m| m.new_size as f64).sum::<f64>() / count as f64;
        let throughput = (avg_size / 1_000_000.0) / (avg_encode / 1000.0);

        report.push_str(&format!(
            "| {} | {} | {:.2}% | {:.2}% | {:.2}% | {:.3} | {:.3} | {:.1} |\n",
            algo,
            count,
            avg_comp * 100.0,
            median_comp * 100.0,
            best_comp * 100.0,
            avg_encode,
            avg_decode,
            throughput
        ));
    }
    report.push('\n');

    // Footer
    report.push_str("---\n\n");
    report.push_str("*generated by gdelta comprehensive benchmark suite*\n");

    std::fs::write(output_path, report)?;
    println!("\n✅ Markdown report generated: {}", output_path);

    Ok(())
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
    println!("✅ JSON report generated: {}", output_path);

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
            .map_or(true, |list| list.contains(&name.to_string()))
    }

    fn should_run_format(&self, name: &str) -> bool {
        self.formats
            .as_ref()
            .map_or(true, |list| list.contains(&name.to_string()))
    }

    fn should_run_pattern(&self, name: &str) -> bool {
        self.change_patterns
            .as_ref()
            .map_or(true, |list| list.contains(&name.to_string()))
    }

    fn should_run_size(&self, name: &str) -> bool {
        self.sizes
            .as_ref()
            .map_or(true, |list| list.contains(&name.to_string()))
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

fn run_benchmarks_with_config(c: &mut Criterion, config: BenchmarkConfig) {
    setup_signal_handler();

    let timestamp = get_timestamp();
    let results_dir = get_results_dir(&timestamp);
    let wal_file = get_wal_file(&timestamp);
    let report_md = get_report_md(&timestamp);
    let report_json = get_report_json(&timestamp);

    println!("📁 Results will be saved with timestamp: {}", timestamp);

    let wal = MetricsWal::new(&wal_file).unwrap();
    let hardware = collect_hardware_info();

    println!("\n🚀 Starting comprehensive delta compression benchmarks...\n");
    println!("💡 Press Ctrl+C to stop early and generate report with collected data\n");
    config.print_info();

    let all_algos: Vec<Box<dyn DeltaAlgorithm>> = vec![
        Box::new(GdeltaAlgorithm),
        Box::new(GdeltaZstdAlgorithm),
        Box::new(GdeltaLz4Algorithm),
        Box::new(XpatchAlgorithm),
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
    println!("📊 Running {} test combinations\n", total_tests);

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
                    print!("\r⏳ Progress: {}/{} ", completed, total_tests);
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
                            })
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
        generate_json_report(all_metrics, hardware.clone(), early_termination, &report_json).unwrap();
    }
}

fn comprehensive_benchmark(c: &mut Criterion) {
    let config = BenchmarkConfig::from_env();
    run_benchmarks_with_config(c, config);
}

criterion_group!(benches, comprehensive_benchmark);
criterion_main!(benches);
