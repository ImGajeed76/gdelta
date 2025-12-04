//! Basic usage example for gdelta.

use gdelta::{decode, encode};

fn main() {
    // Example 1: Simple text modification
    println!("=== Example 1: Simple Text Modification ===");
    let base_text = b"The quick brown fox jumps over the lazy dog";
    let new_text = b"The quick brown cat jumps over the lazy dog";

    match encode(new_text, base_text) {
        Ok(delta) => {
            println!("Base text: {:?}", String::from_utf8_lossy(base_text));
            println!("New text:  {:?}", String::from_utf8_lossy(new_text));
            println!("Base size: {} bytes", base_text.len());
            println!("New size:  {} bytes", new_text.len());
            println!("Delta size: {} bytes", delta.len());
            println!(
                "Compression: {:.1}%",
                (1.0 - delta.len() as f64 / new_text.len() as f64) * 100.0
            );

            // Decode to verify
            match decode(&delta, base_text) {
                Ok(recovered) => {
                    assert_eq!(recovered, new_text);
                    println!("✓ Successfully decoded and verified!");
                }
                Err(e) => eprintln!("Decode error: {}", e),
            }
        }
        Err(e) => eprintln!("Encode error: {}", e),
    }

    println!();

    // Example 2: Large data with small changes
    println!("=== Example 2: Large Data with Small Changes ===");
    let size = 100_000;
    let mut base_data = vec![0u8; size];
    let mut new_data = vec![0u8; size];

    // Fill with pattern
    for i in 0..size {
        base_data[i] = (i % 256) as u8;
        new_data[i] = (i % 256) as u8;
    }

    // Make small modifications (every 500th byte)
    for i in (0..size).step_by(500) {
        new_data[i] = new_data[i].wrapping_add(1);
    }

    match encode(&new_data, &base_data) {
        Ok(delta) => {
            println!("Original size: {} KB", size / 1024);
            println!("Delta size: {} bytes", delta.len());
            println!(
                "Compression ratio: {:.2}x",
                size as f64 / delta.len() as f64
            );

            // Decode to verify
            match decode(&delta, &base_data) {
                Ok(recovered) => {
                    assert_eq!(recovered, new_data);
                    println!("✓ Successfully decoded and verified!");
                }
                Err(e) => eprintln!("Decode error: {}", e),
            }
        }
        Err(e) => eprintln!("Encode error: {}", e),
    }

    println!();

    // Example 3: Document versioning simulation
    println!("=== Example 3: Document Versioning ===");
    let version1 = b"# Project Documentation\n\
                     ## Overview\n\
                     This is the initial version of our project.\n\
                     It contains basic information.\n";

    let version2 = b"# Project Documentation\n\
                     ## Overview\n\
                     This is version 2 of our project.\n\
                     It contains updated information and new features.\n\
                     ## New Section\n\
                     Additional content here.\n";

    match encode(version2, version1) {
        Ok(delta) => {
            println!("Version 1 size: {} bytes", version1.len());
            println!("Version 2 size: {} bytes", version2.len());
            println!("Delta size: {} bytes", delta.len());
            println!(
                "Space saved: {:.1}%",
                (1.0 - delta.len() as f64 / version2.len() as f64) * 100.0
            );

            match decode(&delta, version1) {
                Ok(recovered) => {
                    assert_eq!(recovered, version2);
                    println!("✓ Successfully reconstructed version 2 from delta!");
                }
                Err(e) => eprintln!("Decode error: {}", e),
            }
        }
        Err(e) => eprintln!("Encode error: {}", e),
    }

    println!();
    println!("=== All Examples Completed ===");
}
