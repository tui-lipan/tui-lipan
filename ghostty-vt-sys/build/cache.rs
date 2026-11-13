//! Cache identity for the compiled build recipe and pinned source package.

use sha2::{Digest, Sha256};

/// Embed the sources that produced this executable, rather than reading a possibly newer
/// on-disk recipe. The coverage test requires every Rust file under `build/` to appear here.
pub const RECIPE: &[(&str, &[u8])] = &[
    ("build.rs", include_bytes!("../build.rs")),
    ("build/cache.rs", include_bytes!("cache.rs")),
    ("build/cache_tests.rs", include_bytes!("cache_tests.rs")),
    ("build/patch.rs", include_bytes!("patch.rs")),
    ("src/versions.rs", include_bytes!("../src/versions.rs")),
    // Includes the exact -src dependency pin, and any future build dependencies or features.
    ("Cargo.toml", include_bytes!("../Cargo.toml")),
];

/// Hash named, length-prefixed inputs, so field boundaries and names cannot alias. Use the
/// full digest: old recipe keys and prefixes from a restored cache can never match by name.
pub fn build_key(recipe: &[(&str, &[u8])], inputs: &[(&str, &[u8])]) -> String {
    let mut digest = Sha256::new();
    for (name, bytes) in recipe.iter().chain(inputs) {
        for field in [name.as_bytes(), *bytes] {
            digest.update((field.len() as u64).to_le_bytes());
            digest.update(field);
        }
    }
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
