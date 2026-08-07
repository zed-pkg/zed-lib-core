//! Runs every corpus file in `conformance/cases/` against the Rust
//! implementation. The Dart and TypeScript slices load the same directory, so a
//! case added here is a case all three must answer identically.
//!
//! Rust is also the *oracle* for the generated files
//! (`fuzz-*.json`, written by `cargo run --example generate_fuzz_corpus`), so
//! its job here is narrower but not pointless: it catches the day a change to
//! this crate silently rewrites what the other two are being held to.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use zed_interfaces::registry::PackageMetadata;
use zed_interfaces::vcs::Vcs;
use zed_interfaces::version::VersionScheme;
use zed_lib::{latest_stable, resolve_version};

const RESOLUTION_SCHEMA: &str = "zed-lib/conformance/version-resolution/v1";
const LATEST_SCHEMA: &str = "zed-lib/conformance/latest-stable/v1";

#[derive(Debug, Deserialize)]
struct Corpus {
    schema: String,
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct Case {
    name: String,
    scheme: String,
    versions: Vec<String>,
    /// Present on resolution cases.
    #[serde(default)]
    requirement: Option<String>,
    /// Present on latest-stable cases: what the registry recorded, which the
    /// implementation may not simply forward.
    #[serde(default)]
    latest: Option<String>,
    expect: Expect,
}

#[derive(Debug, Deserialize)]
struct Expect {
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

fn cases_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/cases")
}

fn corpus_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(cases_dir())
        .expect("conformance/cases is readable")
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "the corpus directory must not be empty");
    files
}

/// `latest` is *data* for a latest-stable case — including when it is null,
/// which is what "the registry recorded nothing" looks like. Falling back to
/// the newest version there would quietly answer a different question than the
/// case asked. Resolution cases never read the field, so they get the
/// convenient fallback.
fn metadata(case: &Case, latest_is_data: bool) -> PackageMetadata {
    let latest = if latest_is_data {
        case.latest.clone()
    } else {
        case.latest
            .clone()
            .or_else(|| case.versions.last().cloned())
    };
    PackageMetadata {
        org: "acme".to_string(),
        name: "conformance".to_string(),
        vcs: Vcs::Git,
        repo_url: "https://github.com/acme/conformance".to_string(),
        description: None,
        latest,
        versions: case.versions.clone(),
        version_scheme: VersionScheme::from_str_lenient(&case.scheme),
        tags: Vec::new(),
    }
}

fn check_resolution(case: &Case, file: &str) {
    let meta = metadata(case, false);
    let requirement = case.requirement.as_deref().unwrap_or_else(|| {
        panic!(
            "{file}:{}: resolution cases need a `requirement`",
            case.name
        )
    });
    match (&case.expect.version, &case.expect.error) {
        (Some(want), None) => {
            let got = resolve_version(&meta, requirement).unwrap_or_else(|error| {
                panic!("{file}:{}: expected {want}, got {error}", case.name)
            });
            assert_eq!(got, want, "{file}:{}", case.name);
        }
        (None, Some(want)) => match resolve_version(&meta, requirement) {
            Ok(version) => panic!("{file}:{}: expected {want}, resolved {version}", case.name),
            Err(error) => assert_eq!(error.kind(), want, "{file}:{}", case.name),
        },
        _ => panic!(
            "{file}:{}: declare exactly one of `version` or `error`",
            case.name
        ),
    }
}

fn check_latest(case: &Case, file: &str) {
    assert!(
        case.expect.error.is_none(),
        "{file}:{}: latest-stable cases cannot fail, they return null",
        case.name
    );
    let meta = metadata(case, true);
    assert_eq!(
        latest_stable(&meta),
        case.expect.version.as_deref(),
        "{file}:{}",
        case.name
    );
}

#[test]
fn every_corpus_file_passes() {
    let mut total = 0;
    for path in corpus_files() {
        let file = path.file_name().unwrap().to_string_lossy().to_string();
        let raw = fs::read_to_string(&path).expect("corpus file is readable");
        let corpus: Corpus = serde_json::from_str(&raw)
            .unwrap_or_else(|error| panic!("{file} is not a valid corpus: {error}"));
        assert!(!corpus.cases.is_empty(), "{file} has no cases");

        for case in &corpus.cases {
            match corpus.schema.as_str() {
                RESOLUTION_SCHEMA => check_resolution(case, &file),
                LATEST_SCHEMA => check_latest(case, &file),
                other => panic!("{file}: unknown corpus schema `{other}`"),
            }
            total += 1;
        }
    }
    // A loader bug that silently matched nothing would otherwise look like a
    // clean run in every language at once.
    assert!(
        total > 100,
        "expected the generated corpus too, ran only {total}"
    );
    println!("ran {total} cases");
}

#[test]
fn the_generated_corpus_is_present_and_deterministic() {
    let names: Vec<String> = corpus_files()
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    for expected in [
        "version-resolution.json",
        "latest-stable.json",
        "fuzz-version-resolution.json",
        "fuzz-latest-stable.json",
    ] {
        assert!(
            names.iter().any(|name| name == expected),
            "missing corpus file {expected}; run `cargo run --example generate_fuzz_corpus`"
        );
    }
}
