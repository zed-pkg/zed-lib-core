//! This repository's own `.zpkg.toml` must satisfy the rules `zed-interfaces`
//! defines. zed-lib is a polyglot package with three implementation slices and
//! a corpus target; nothing else in CI parses that manifest, so a mistake in it
//! would only surface at publish time.

use std::path::Path;

use zed_interfaces::manifest::Manifest;

fn own_manifest() -> Manifest {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.zpkg.toml");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    Manifest::parse(&raw).expect("the repository manifest parses and validates")
}

#[test]
fn the_repository_manifest_is_valid() {
    let manifest = own_manifest();
    assert_eq!(manifest.package.name, "zed-lib");
    assert!(manifest.is_polyglot());
}

#[test]
fn every_implementation_slice_is_its_own_target_with_an_isolated_root() {
    let manifest = own_manifest();
    for (target, dir, adapter) in [
        ("rust", "src/rust", "rust"),
        ("dart", "src/dart", "dart"),
        ("typescript", "src/ts", "node"),
    ] {
        let section = manifest
            .targets
            .get(target)
            .unwrap_or_else(|| panic!("missing `[targets.{target}]`"));
        assert_eq!(section.dir, dir, "target `{target}` moved");
        assert_eq!(section.adapter.as_deref(), Some(adapter));
        assert!(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(dir)
                .is_dir(),
            "target `{target}` points at `{dir}`, which does not exist"
        );
    }
}

#[test]
fn the_corpus_ships_as_its_own_target() {
    let manifest = own_manifest();
    let corpus = manifest
        .targets
        .get("conformance")
        .expect("the corpus is consumable on its own");
    assert_eq!(corpus.dir, "conformance");
    // An implementation in a language zed-lib does not ship yet still needs
    // something to be correct against, so the corpus must not be Rust-only.
    assert_eq!(corpus.adapter.as_deref(), Some("none"));
}
