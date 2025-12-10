//! gdelta CLI - Fast delta compression tool
//!
//! Usage:
//!   gdelta encode <base> <new> -o <output> [OPTIONS]
//!   gdelta decode <base> <delta> -o <output> [OPTIONS]

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use owo_colors::OwoColorize;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::time::Instant;
use sysinfo::System;

/// Fast delta compression tool
#[derive(Parser)]
#[command(name = "gdelta")]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a delta patch from base to new file
    Encode {
        /// Base file (original version)
        base: PathBuf,

        /// New file (target version)
        new: PathBuf,

        /// Output delta file
        #[arg(short, long)]
        output: PathBuf,

        /// Compression method
        #[arg(short, long, value_enum, default_value = "none")]
        compress: Compression,

        /// Verify delta after creation by decoding and comparing
        #[arg(short, long)]
        verify: bool,

        /// Skip memory warning prompt
        #[arg(short = 'y', long)]
        yes: bool,

        /// Overwrite output file if it exists
        #[arg(short, long)]
        force: bool,

        /// Suppress output except errors
        #[arg(short, long)]
        quiet: bool,
    },
    /// Apply a delta patch to reconstruct the new file
    Decode {
        /// Base file (original version)
        base: PathBuf,

        /// Delta patch file
        delta: PathBuf,

        /// Output file
        #[arg(short, long)]
        output: PathBuf,

        /// Compression format (auto-detected by default)
        #[arg(long, value_enum)]
        format: Option<Compression>,

        /// Skip memory warning prompt
        #[arg(short = 'y', long)]
        yes: bool,

        /// Overwrite output file if it exists
        #[arg(short, long)]
        force: bool,

        /// Suppress output except errors
        #[arg(short, long)]
        quiet: bool,
    },
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum, Debug)]
enum Compression {
    /// No compression (raw delta)
    None,
    /// Zstd compression (good balance)
    Zstd,
    /// LZ4 compression (faster)
    Lz4,
}

// Exit codes
const EXIT_SUCCESS: i32 = 0;
const EXIT_ERROR: i32 = 1;
const EXIT_ENCODE_DECODE_FAILED: i32 = 2;
const EXIT_OUT_OF_MEMORY: i32 = 4;
const EXIT_USER_CANCELLED: i32 = 5;

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Encode {
            base,
            new,
            output,
            compress,
            verify,
            yes,
            force,
            quiet,
        } => handle_encode(&base, &new, &output, compress, verify, yes, force, quiet),
        Commands::Decode {
            base,
            delta,
            output,
            format,
            yes,
            force,
            quiet,
        } => handle_decode(&base, &delta, &output, format, yes, force, quiet),
    };

    match result {
        Ok(()) => process::exit(EXIT_SUCCESS),
        Err(e) => {
            eprintln!("{} {}", "Error:".bright_red().bold(), e);

            // Determine exit code based on error message
            let exit_code = if e.to_string().contains("out of memory")
                || e.to_string().contains("Out of memory")
                || e.to_string().contains("Insufficient memory")
            {
                EXIT_OUT_OF_MEMORY
            } else if e.to_string().contains("cancelled")
                || e.to_string().contains("Cancelled")
            {
                EXIT_USER_CANCELLED
            } else if e.to_string().contains("encode") || e.to_string().contains("decode") {
                EXIT_ENCODE_DECODE_FAILED
            } else {
                EXIT_ERROR
            };

            process::exit(exit_code);
        }
    }
}

fn handle_encode(
    base_path: &Path,
    new_path: &Path,
    output_path: &Path,
    compress: Compression,
    verify: bool,
    yes: bool,
    force: bool,
    quiet: bool,
) -> Result<()> {
    // Check if files exist
    if !base_path.exists() {
        bail!("File not found: {}", base_path.display());
    }
    if !new_path.exists() {
        bail!("File not found: {}", new_path.display());
    }

    // Check if output exists
    if output_path.exists() && !force {
        bail!(
            "Output file already exists: {}\n   Use --force to overwrite",
            output_path.display()
        );
    }

    // Get file sizes
    let base_size = fs::metadata(base_path)
        .context("Failed to read base file metadata")?
        .len();
    let new_size = fs::metadata(new_path)
        .context("Failed to read new file metadata")?
        .len();

    if !quiet {
        println!(
            "{} Base: {}, New: {}",
            "File sizes:".bright_cyan(),
            format_bytes(base_size),
            format_bytes(new_size)
        );
    }

    // Memory check
    let required = estimate_encode_memory(base_size, new_size);
    check_memory(required, yes, quiet)?;

    // Read files
    if !quiet {
        let total_steps = if verify { 4 } else { 3 };
        println!("{} Reading files...", format!("Step 1/{}:", total_steps).bright_cyan());
    }

    let base_data = fs::read(base_path)
        .with_context(|| format!("Failed to read base file: {}", base_path.display()))?;
    let new_data = fs::read(new_path)
        .with_context(|| format!("Failed to read new file: {}", new_path.display()))?;

    // Encode
    if !quiet {
        let total_steps = if verify { 4 } else { 3 };
        println!("{} Encoding delta...", format!("Step 2/{}:", total_steps).bright_cyan());
    }

    let start = Instant::now();
    let delta = gdelta::encode(&new_data, &base_data)
        .map_err(|e| anyhow::anyhow!("Encode failed: {}", e))?;
    let encode_time = start.elapsed();

    // Compress if requested
    let (final_delta, compression_time) = if compress != Compression::None {
        if !quiet {
            let total_steps = if verify { 4 } else { 3 };
            println!(
                "{} Compressing with {:?}...",
                format!("Step 2.5/{}:", total_steps).bright_cyan(),
                compress
            );
        }

        let start = Instant::now();
        let compressed = match compress {
            Compression::Zstd => compress_zstd(&delta)?,
            Compression::Lz4 => compress_lz4(&delta)?,
            Compression::None => unreachable!(),
        };
        let time = start.elapsed();
        (compressed, Some(time))
    } else {
        (delta, None)
    };

    // Write output
    if !quiet {
        let total_steps = if verify { 4 } else { 3 };
        println!("{} Writing output...", format!("Step 3/{}:", total_steps).bright_cyan());
    }

    fs::write(output_path, &final_delta)
        .with_context(|| format!("Failed to write output file: {}", output_path.display()))?;

    // Verify if requested
    let verify_result = if verify {
        if !quiet {
            println!("{} Verifying delta...", "Step 4/4:".bright_cyan());
        }

        let verify_start = Instant::now();

        // Decompress if needed
        let delta_for_verify = if compress != Compression::None {
            decompress_if_needed(&final_delta, Some(compress), true)?.0
        } else {
            final_delta.clone()
        };

        // Decode
        let reconstructed = gdelta::decode(&delta_for_verify, &base_data)
            .map_err(|e| anyhow::anyhow!("Verification decode failed: {}", e))?;

        let verify_time = verify_start.elapsed();

        // Compare
        if reconstructed != new_data {
            bail!(
                "Verification failed: reconstructed output does not match original new file\n   \
                 Expected {} bytes, got {} bytes",
                new_data.len(),
                reconstructed.len()
            );
        }

        Some(verify_time)
    } else {
        None
    };

    // Success message
    if !quiet {
        println!();
        println!(
            "{} Created {} ({}, {:.1}% of new file)",
            "Success:".bright_green().bold(),
            output_path.display(),
            format_bytes(final_delta.len() as u64),
            (final_delta.len() as f64 / new_size as f64) * 100.0
        );
        print!("   Encoding took {}", format_duration(encode_time));
        if let Some(comp_time) = compression_time {
            print!(", compression took {}", format_duration(comp_time));
        }
        if let Some(verify_time) = verify_result {
            print!(", verification took {}", format_duration(verify_time));
        }
        println!();
    }

    Ok(())
}

fn handle_decode(
    base_path: &Path,
    delta_path: &Path,
    output_path: &Path,
    format_override: Option<Compression>,
    yes: bool,
    force: bool,
    quiet: bool,
) -> Result<()> {
    // Check if files exist
    if !base_path.exists() {
        bail!("File not found: {}", base_path.display());
    }
    if !delta_path.exists() {
        bail!("File not found: {}", delta_path.display());
    }

    // Check if output exists
    if output_path.exists() && !force {
        bail!(
            "Output file already exists: {}\n   Use --force to overwrite",
            output_path.display()
        );
    }

    // Get file sizes
    let base_size = fs::metadata(base_path)
        .context("Failed to read base file metadata")?
        .len();
    let delta_size = fs::metadata(delta_path)
        .context("Failed to read delta file metadata")?
        .len();

    if !quiet {
        println!(
            "{} Base: {}, Delta: {}",
            "File sizes:".bright_cyan(),
            format_bytes(base_size),
            format_bytes(delta_size)
        );
    }

    // Memory check (estimate output size as ~base_size)
    let required = estimate_decode_memory(base_size, delta_size);
    check_memory(required, yes, quiet)?;

    // Read files
    if !quiet {
        println!("{} Reading files...", "Step 1/3:".bright_cyan());
    }

    let base_data = fs::read(base_path)
        .with_context(|| format!("Failed to read base file: {}", base_path.display()))?;
    let delta_data = fs::read(delta_path)
        .with_context(|| format!("Failed to read delta file: {}", delta_path.display()))?;

    // Detect or use specified compression
    let (delta_decompressed, detected_format, decompression_time) =
        decompress_if_needed(&delta_data, format_override, quiet)?;

    if !quiet && detected_format != Compression::None {
        println!(
            "{} Detected {:?} compression",
            "Info:".bright_cyan(),
            detected_format
        );
    }

    // Decode
    if !quiet {
        println!("{} Decoding delta...", "Step 2/3:".bright_cyan());
    }

    let start = Instant::now();
    let output_data = gdelta::decode(&delta_decompressed, &base_data)
        .map_err(|e| anyhow::anyhow!("Decode failed: {}", e))?;
    let decode_time = start.elapsed();

    // Write output
    if !quiet {
        println!("{} Writing output...", "Step 3/3:".bright_cyan());
    }

    fs::write(output_path, &output_data)
        .with_context(|| format!("Failed to write output file: {}", output_path.display()))?;

    // Success message
    if !quiet {
        println!();
        println!(
            "{} Created {} ({})",
            "Success:".bright_green().bold(),
            output_path.display(),
            format_bytes(output_data.len() as u64)
        );
        print!("   Decoding took {}", format_duration(decode_time));
        if let Some(decomp_time) = decompression_time {
            print!(", decompression took {}", format_duration(decomp_time));
        }
        println!();
    }

    Ok(())
}

// ============================================================================
// Memory Management
// ============================================================================

fn estimate_encode_memory(base_size: u64, new_size: u64) -> u64 {
    // base + new + delta (worst case = new) + 20% overhead
    base_size + new_size + new_size + (base_size / 5)
}

fn estimate_decode_memory(base_size: u64, delta_size: u64) -> u64 {
    // base + delta + output (estimate as base) + 20% overhead
    base_size + delta_size + base_size + (base_size / 5)
}

fn check_memory(required: u64, skip_prompt: bool, quiet: bool) -> Result<()> {
    let mut sys = System::new_all();
    sys.refresh_memory();

    let available = sys.available_memory();
    let total = sys.total_memory();

    // Check if totally insufficient (even if all apps closed)
    if required > total {
        bail!(
            "Insufficient memory\n   Required: ~{}\n   Total RAM: {}\n\n   \
             These files cannot be processed on this system.",
            format_bytes(required),
            format_bytes(total)
        );
    }

    // Calculate usage percentage
    let usage_pct = (required as f64 / available as f64) * 100.0;

    // Show status if not quiet
    if !quiet && usage_pct < 80.0 {
        println!(
            "{} ~{} required, {} available {}",
            "Memory:".bright_cyan(),
            format_bytes(required),
            format_bytes(available),
            "✓".bright_green()
        );
    }

    // Warn if high memory usage
    if usage_pct >= 80.0 {
        eprintln!();
        eprintln!(
            "{} This operation requires ~{}",
            "Memory warning:".bright_yellow().bold(),
            format_bytes(required)
        );
        eprintln!(
            "   Available: {} free ({} total)",
            format_bytes(available),
            format_bytes(total)
        );
        eprintln!();

        if usage_pct >= 100.0 {
            eprintln!("   Loading these files will use {:.0}% of available memory.", usage_pct);
            eprintln!("   {}", "Your system may freeze or crash.".bright_red().bold());
        } else {
            eprintln!("   Loading these files will use {:.0}% of available memory.", usage_pct);
            eprintln!("   System may slow down temporarily.");
        }
        eprintln!();

        if skip_prompt {
            eprintln!("   {} Continuing anyway (--yes flag)", "⚠".bright_yellow());
            eprintln!();
        } else {
            eprint!("   Continue? [y/N]: ");
            io::stderr().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;

            if !input.trim().eq_ignore_ascii_case("y") {
                bail!("Cancelled by user");
            }
            eprintln!();
        }
    }

    Ok(())
}

// ============================================================================
// Compression/Decompression
// ============================================================================

fn compress_zstd(data: &[u8]) -> Result<Vec<u8>> {
    zstd::encode_all(data, 3).context("Zstd compression failed")
}

fn compress_lz4(data: &[u8]) -> Result<Vec<u8>> {
    // Use LZ4 frame format for proper magic bytes
    let mut compressed = Vec::new();
    let mut encoder = lz4::EncoderBuilder::new()
        .level(1) // Fast compression
        .build(&mut compressed)
        .context("Failed to create LZ4 encoder")?;

    io::copy(&mut &data[..], &mut encoder)
        .context("Failed to compress with LZ4")?;

    let (_output, result) = encoder.finish();
    result.context("Failed to finish LZ4 compression")?;

    Ok(compressed)
}

fn decompress_if_needed(
    data: &[u8],
    format_override: Option<Compression>,
    quiet: bool,
) -> Result<(Vec<u8>, Compression, Option<std::time::Duration>)> {
    // If format is explicitly specified, use it
    if let Some(format) = format_override {
        let start = Instant::now();
        let decompressed = match format {
            Compression::None => return Ok((data.to_vec(), Compression::None, None)),
            Compression::Zstd => {
                if !quiet {
                    println!("{} Decompressing with Zstd...", "Step 1.5/3:".bright_cyan());
                }
                zstd::decode_all(data).context("Zstd decompression failed")?
            }
            Compression::Lz4 => {
                if !quiet {
                    println!("{} Decompressing with LZ4...", "Step 1.5/3:".bright_cyan());
                }
                decompress_lz4(data)?
            }
        };
        let time = start.elapsed();
        return Ok((decompressed, format, Some(time)));
    }

    // Auto-detect compression by magic bytes
    const ZSTD_MAGIC: &[u8] = &[0x28, 0xB5, 0x2F, 0xFD];
    const LZ4_MAGIC: &[u8] = &[0x04, 0x22, 0x4D, 0x18];

    if data.starts_with(ZSTD_MAGIC) {
        if !quiet {
            println!("{} Decompressing (detected Zstd)...", "Step 1.5/3:".bright_cyan());
        }
        let start = Instant::now();
        let decompressed = zstd::decode_all(data)
            .context("Zstd decompression failed")?;
        let time = start.elapsed();
        Ok((decompressed, Compression::Zstd, Some(time)))
    } else if data.starts_with(LZ4_MAGIC) {
        if !quiet {
            println!("{} Decompressing (detected LZ4)...", "Step 1.5/3:".bright_cyan());
        }
        let start = Instant::now();
        let decompressed = decompress_lz4(data)?;
        let time = start.elapsed();
        Ok((decompressed, Compression::Lz4, Some(time)))
    } else {
        // No compression detected
        Ok((data.to_vec(), Compression::None, None))
    }
}

fn decompress_lz4(data: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = lz4::Decoder::new(data)
        .context("Failed to create LZ4 decoder")?;

    let mut decompressed = Vec::new();
    io::copy(&mut decoder, &mut decompressed)
        .context("Failed to decompress LZ4 data")?;

    Ok(decompressed)
}

// ============================================================================
// Utilities
// ============================================================================

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

fn format_duration(duration: std::time::Duration) -> String {
    let nanos = duration.as_nanos();

    if nanos < 1_000 {
        format!("{}ns", nanos)
    } else if nanos < 1_000_000 {
        format!("{:.1}μs", nanos as f64 / 1_000.0)
    } else if nanos < 1_000_000_000 {
        format!("{:.2}ms", nanos as f64 / 1_000_000.0)
    } else {
        format!("{:.3}s", duration.as_secs_f64())
    }
}