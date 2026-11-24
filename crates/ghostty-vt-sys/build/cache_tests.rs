//! Build-cache tests run by `cargo test -p tui-lipan-ghostty-vt-sys` without spawning Cargo or Zig.

use std::collections::BTreeSet;
use std::path::Path;

mod cache;

#[test]
fn compiled_recipe_covers_every_rust_build_source() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = BTreeSet::from(["build.rs".to_owned()]);
    collect_rust_sources(root, &root.join("build"), &mut files);
    let embedded: BTreeSet<_> = cache::RECIPE
        .iter()
        .filter(|(name, _)| *name == "build.rs" || name.starts_with("build/"))
        .map(|(name, _)| (*name).to_owned())
        .collect();
    assert_eq!(
        embedded, files,
        "add every new build helper to the compiled recipe hash"
    );
    for (name, contents) in cache::RECIPE {
        assert_eq!(std::fs::read(root.join(name)).unwrap(), *contents, "{name}");
    }
}

fn collect_rust_sources(root: &Path, dir: &Path, files: &mut BTreeSet<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_rust_sources(root, &path, files);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.insert(
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
}

#[test]
fn changing_a_build_flag_invalidates_the_key_without_a_version_or_patch_change() {
    let original = cache::RECIPE
        .iter()
        .find(|(name, _)| *name == "build.rs")
        .unwrap()
        .1;
    let original = std::str::from_utf8(original).unwrap();
    let changed = original.replace(".arg(\"-Dstrip=true\")", ".arg(\"-Dstrip=false\")");
    assert_ne!(
        original, changed,
        "the negative control must change an actual build flag"
    );
    let recipe: Vec<_> = cache::RECIPE
        .iter()
        .map(|&(name, bytes)| {
            (
                name,
                if name == "build.rs" {
                    changed.as_bytes()
                } else {
                    bytes
                },
            )
        })
        .collect();
    let unchanged = [("version", b"1.0.1".as_slice()), ("patch", b"same patch")];
    assert_ne!(
        cache::build_key(cache::RECIPE, &unchanged),
        cache::build_key(&recipe, &unchanged)
    );
}

#[test]
fn source_version_patch_manifest_and_configuration_each_change_the_key() {
    let inputs: &[(&str, &[u8])] = &[
        ("version", b"1.0.1"),
        ("target", b"native"),
        ("optimize", b"ReleaseFast"),
        ("cpu", b"baseline"),
        ("patch", b"patch digest"),
        (
            "source-manifest",
            b"version = \"0.1.0+libghostty-vt.1.0.1\"",
        ),
        ("source-patches", b"alternate-resize.patch digest\n"),
    ];
    let original = cache::build_key(cache::RECIPE, inputs);
    assert_eq!(original, cache::build_key(cache::RECIPE, inputs));
    for index in 0..inputs.len() {
        let changed = [inputs[index].1, b"changed"].concat();
        let mut modified = inputs.to_vec();
        modified[index].1 = &changed;
        assert_ne!(
            original,
            cache::build_key(cache::RECIPE, &modified),
            "{}",
            inputs[index].0
        );
    }
}

#[test]
fn input_boundaries_are_part_of_the_digest() {
    let a = [("a", b"bc".as_slice()), ("d", b"ef".as_slice())];
    let b = [("a", b"b".as_slice()), ("cd", b"ef".as_slice())];
    assert_ne!(cache::build_key(&[], &a), cache::build_key(&[], &b));
}
