//! Integration tests for gdelta.

use gdelta::{decode, encode};

#[test]
fn test_basic_encode_decode() {
    let base = b"Hello, World!";
    let new = b"Hello, Rust!";

    let delta = encode(new, base).unwrap();
    let recovered = decode(&delta, base).unwrap();

    assert_eq!(recovered, new);
}

#[test]
fn test_identical_data() {
    let data = b"Identical data on both sides";

    let delta = encode(data, data).unwrap();
    let recovered = decode(&delta, data).unwrap();

    assert_eq!(recovered, data);
}

#[test]
fn test_completely_different() {
    let base = b"AAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    let new = b"BBBBBBBBBBBBBBBBBBBBBBBBBBBB";

    let delta = encode(new, base).unwrap();
    let recovered = decode(&delta, base).unwrap();

    assert_eq!(recovered, new);
}

#[test]
fn test_empty_new_data() {
    let base = b"Some base data here";
    let new = b"";

    let delta = encode(new, base).unwrap();
    let recovered = decode(&delta, base).unwrap();

    assert_eq!(recovered, new);
}

#[test]
fn test_empty_base_data() {
    let base = b"";
    let new = b"Some new data here";

    let delta = encode(new, base).unwrap();
    let recovered = decode(&delta, base).unwrap();

    assert_eq!(recovered, new);
}

#[test]
fn test_large_data() {
    // Create 100KB of data with patterns
    let mut base = vec![0u8; 100_000];
    let mut new = vec![0u8; 100_000];

    for i in 0..base.len() {
        base[i] = (i % 256) as u8;
        new[i] = (i % 256) as u8;
    }

    // Make some modifications
    for i in (0..new.len()).step_by(488) {
        if i < new.len() {
            new[i] = new[i].wrapping_add(1);
        }
    }

    let delta = encode(&new, &base).unwrap();
    let recovered = decode(&delta, &base).unwrap();

    assert_eq!(recovered, new);

    // Delta should be smaller than new data
    assert!(delta.len() < new.len());

    println!("Original size: {} bytes", new.len());
    println!("Delta size: {} bytes", delta.len());
    println!(
        "Compression ratio: {:.2}x",
        new.len() as f64 / delta.len() as f64
    );
}

#[test]
fn test_text_similarity() {
    let base = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. \
                Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. \
                Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris.";

    let new = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. \
               Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. \
               Ut enim ad maxim veniam, quis nostrud exercitation ullamco laboris.";

    let delta = encode(new.as_bytes(), base.as_bytes()).unwrap();
    let recovered = decode(&delta, base.as_bytes()).unwrap();

    assert_eq!(recovered, new.as_bytes());
    assert!(delta.len() < new.len());
}

#[test]
fn test_prefix_only() {
    let base = b"Hello, World! This is a test.";
    let new = b"Hello, World! This is different.";

    let delta = encode(new, base).unwrap();
    let recovered = decode(&delta, base).unwrap();

    assert_eq!(recovered, new);
}

#[test]
fn test_suffix_only() {
    let base = b"Start is different. Common ending.";
    let new = b"Beginning differs. Common ending.";

    let delta = encode(new, base).unwrap();
    let recovered = decode(&delta, base).unwrap();

    assert_eq!(recovered, new);
}

#[test]
fn test_middle_insertion() {
    let base = b"The quick fox jumps.";
    let new = b"The quick brown fox jumps.";

    let delta = encode(new, base).unwrap();
    let recovered = decode(&delta, base).unwrap();

    assert_eq!(recovered, new);
}

#[test]
fn test_middle_deletion() {
    let base = b"The quick brown fox jumps.";
    let new = b"The quick fox jumps.";

    let delta = encode(new, base).unwrap();
    let recovered = decode(&delta, base).unwrap();

    assert_eq!(recovered, new);
}

#[test]
fn test_repeated_pattern() {
    let base = b"ABCABCABCABCABCABCABCABC";
    let new = b"ABCABCABCXYZABCABCABCABC";

    let delta = encode(new, base).unwrap();
    let recovered = decode(&delta, base).unwrap();

    assert_eq!(recovered, new);
}

#[test]
fn test_binary_data() {
    let base: Vec<u8> = (0..=255).cycle().take(1000).collect();
    let mut new = base.clone();

    // Modify some bytes
    new[100] = 99;
    new[500] = 88;
    new[900] = 77;

    let delta = encode(&new, &base).unwrap();
    let recovered = decode(&delta, &base).unwrap();

    assert_eq!(recovered, new);
}
