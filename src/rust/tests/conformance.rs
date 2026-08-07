//! Runs the shared conformance corpus against the Rust implementation.
//!
//! The corpus is language-neutral on purpose: the Dart and TypeScript slices
//! of zed-lib will run the same file, so "the CLI resolved 1.4.0 but the web UI
//! showed 2.0.0" becomes a failing test in one of them rather than a support
//! ticket.

use std::path::Path;

use serde::Deserialize;
use zed_interfaces::registry::PackageMetadata;
use zed_interfaces::vcs::Vcs;
use zed_interfaces::version::VersionScheme;
use zed_lib::resolve_version;

#[derive(Debug, Deserialize)]
struct Corpus {
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct Case {
    name: String,
    scheme: String,
    versions: Vec<String>,
    requirement: String,
    expect: Expect,
}

#[derive(Debug, Deserialize)]
struct Expect {
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

fn corpus() -> Corpus {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance/cases/version-resolution.json");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    serde_json::from_str(&raw).expect("the corpus is valid JSON in the expected shape")
}

#[test]
fn the_shared_corpus_passes() {
    let corpus = corpus();
    assert!(!corpus.cases.is_empty(), "the corpus must not be empty");

    for case in &corpus.cases {
        let metadata = PackageMetadata {
            org: "acme".to_string(),
            name: "conformance".to_string(),
            vcs: Vcs::Git,
            repo_url: "https://github.com/acme/conformance".to_string(),
            description: None,
            latest: case.versions.last().cloned(),
            versions: case.versions.clone(),
            version_scheme: VersionScheme::from_str_lenient(&case.scheme),
            tags: Vec::new(),
        };

        match (&case.expect.version, &case.expect.error) {
            (Some(want), None) => {
                let got = resolve_version(&metadata, &case.requirement).unwrap_or_else(|error| {
                    panic!("{}: expected {want}, got error {error}", case.name)
                });
                assert_eq!(got, want, "{}", case.name);
            }
            (None, Some(want)) => {
                let error =
                    resolve_version(&metadata, &case.requirement).unwrap_err_or_panic(&case.name);
                assert_eq!(error.kind(), want, "{}", case.name);
            }
            _ => panic!(
                "{}: a case declares exactly one of `version` or `error`",
                case.name
            ),
        }
    }
}

/// `Result::unwrap_err` needs `T: Debug`, which `&str` satisfies — but the
/// panic message would not say which case failed.
trait UnwrapErrOrPanic<E> {
    fn unwrap_err_or_panic(self, case: &str) -> E;
}

impl<T: std::fmt::Debug, E> UnwrapErrOrPanic<E> for Result<T, E> {
    fn unwrap_err_or_panic(self, case: &str) -> E {
        match self {
            Ok(value) => panic!("{case}: expected an error, resolved {value:?}"),
            Err(error) => error,
        }
    }
}
