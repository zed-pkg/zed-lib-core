//! Production Rust resolver checks against independent selection invariants.
//! No registry I/O or .atto compiler execution is implied by these tests.
use zed_interfaces::registry::PackageMetadata;
use zed_interfaces::version::VersionScheme;
use zed_lib::{latest_stable, requirement_matches, resolve_version};

fn metadata(scheme: VersionScheme, versions: &[&str]) -> PackageMetadata {
    serde_json::from_value(serde_json::json!({
        "org": "audit-example",
        "name": "sample-lib",
        "vcs": "git",
        "repo_url": "https://example.invalid/sample-lib",
        "description": null,
        "latest": versions.first(),
        "versions": versions,
        "version_scheme": scheme,
        "tags": []
    }))
    .expect("bounded fixture must match interface schema")
}

#[test]
fn resolution_is_independent_of_published_version_order() {
    let candidates = ["1.0.0", "1.2.0", "1.9.0", "2.0.0", "1.7.1"];
    let mut checks = 0;
    for rotation in 0..candidates.len() {
        for reversed in [false, true] {
            let mut versions = candidates.to_vec();
            versions.rotate_left(rotation);
            if reversed {
                versions.reverse();
            }
            let meta = metadata(VersionScheme::Semver, &versions);
            assert_eq!(resolve_version(&meta, "^1.0").unwrap(), "1.9.0");
            assert_eq!(latest_stable(&meta), Some("2.0.0"));
            checks += 1;
        }
    }
    assert_eq!(checks, 10);
}

#[test]
fn opaque_exact_tags_and_ranges_obey_pinned_contract() {
    let published = ["2026.07.24", "release-a", "release-b"];
    let meta = metadata(VersionScheme::Opaque, &published);
    for tag in published {
        assert_eq!(resolve_version(&meta, tag).unwrap(), tag);
        assert!(requirement_matches(VersionScheme::Opaque, tag, tag));
    }
    for range in ["^1.0", "~1.0", ">=1.0", "*"] {
        assert_eq!(
            resolve_version(&meta, range).unwrap_err().kind(),
            "invalid_requirement",
        );
        for published_tag in published {
            assert!(!requirement_matches(
                VersionScheme::Opaque,
                range,
                published_tag,
            ));
        }
    }
    // The registry's stale latest pointer is never silently trusted.
    let mut withdrawn = meta;
    withdrawn.latest = Some("withdrawn".to_owned());
    assert_eq!(latest_stable(&withdrawn), None);
}

#[test]
fn invalid_requirement_and_no_match_are_distinct_fail_closed_outcomes() {
    let meta = metadata(VersionScheme::Semver, &["1.0.0", "1.2.0"]);
    for malformed in ["^1.x.y", "1.x.y", "1.2.3.4"] {
        assert_eq!(
            resolve_version(&meta, malformed).unwrap_err().kind(),
            "invalid_requirement"
        );
    }
    assert_eq!(
        resolve_version(&meta, "^9.0").unwrap_err().kind(),
        "unsatisfied"
    );
    let empty = metadata(VersionScheme::Semver, &[]);
    assert_eq!(
        resolve_version(&empty, "^1.0").unwrap_err().kind(),
        "no_versions"
    );
}

#[test]
fn negative_control_detects_first_published_instead_of_highest_matching() {
    let meta = metadata(VersionScheme::Semver, &["1.0.0", "1.9.0", "1.4.0"]);
    let correct = resolve_version(&meta, "^1.0").unwrap();
    let broken = meta
        .versions
        .iter()
        .find(|version| requirement_matches(VersionScheme::Semver, "^1.0", version))
        .expect("bounded mutation should match");
    assert_ne!(
        correct, broken,
        "negative control failed to catch first-match resolution"
    );
    assert_eq!(correct, "1.9.0");
}
