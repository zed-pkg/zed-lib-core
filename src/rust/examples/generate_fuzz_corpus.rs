//! Generates a differential-test corpus: pseudo-random (scheme, versions,
//! requirement) triples, answered by the **Rust** implementation and written as
//! ordinary corpus cases that Dart and TypeScript then have to reproduce.
//!
//! Rust is the reference on purpose. It is the slice consumers already run
//! through `zed-cli`, and it delegates the hard part to the `semver` crate that
//! Cargo itself uses — so "the hand-written Dart and TypeScript algebra agrees
//! with Cargo" is the property under test. The 20-odd hand-written cases pin
//! the decisions a human reasoned about; this pins the thousand combinations
//! nobody would think to write down.
//!
//! Deterministic by construction (a fixed-seed LCG, no clock, no entropy), so
//! regenerating produces byte-identical output and CI can diff it.
//!
//! Run with: `cargo run --locked --example generate_fuzz_corpus`

use std::fs;
use std::path::Path;

use zed_interfaces::registry::PackageMetadata;
use zed_interfaces::vcs::Vcs;
use zed_interfaces::version::VersionScheme;
use zed_lib::{latest_stable, resolve_version};

/// Deterministic LCG (Numerical Recipes constants). A real RNG would make the
/// corpus unreproducible, which would turn a CI diff into noise.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0 >> 16
    }

    fn pick<'a, T>(&mut self, options: &'a [T]) -> &'a T {
        &options[(self.next() as usize) % options.len()]
    }

    fn range(&mut self, low: usize, high: usize) -> usize {
        low + (self.next() as usize) % (high - low + 1)
    }
}

const VERSION_POOL: &[&str] = &[
    "0.0.1",
    "0.1.0",
    "0.1.9",
    "0.2.0",
    "1.0.0",
    "1.0.1",
    "1.2.0",
    "1.2.3",
    "1.2.9",
    "1.3.0",
    "1.9.9",
    "2.0.0",
    "2.1.0",
    "10.0.0",
    "1.0.0-rc.1",
    "2.0.0-alpha.1",
    "1.2.3-beta.2",
    "v1.0.0",
    "v2.0.0+incompatible",
    "2026.01.01",
    "2026.06.01",
    "2026.07.24",
    "2026.10.01",
    "1.2.3rc1",
    "1.2.3.post1",
    "legacy-api",
    "release-candidate-1",
    "nightly",
];

const REQUIREMENT_POOL: &[&str] = &[
    "^1",
    "^1.2",
    "^1.2.3",
    "^0.1",
    "^0.0.1",
    "^2",
    "^2026",
    "~1",
    "~1.2",
    "~1.2.3",
    ">=1.2",
    ">=1.2 <2",
    ">1.0.0",
    "<2.0.0",
    "<=1.2.3",
    "=1.0.0",
    "=1.2",
    "*",
    "1",
    "1.2",
    "1.2.3",
    "1.*",
    "1.2.*",
    "1.x",
    "2026.07.24",
    ">=2026.7",
    "legacy-api",
    "nightly",
    "^1.x.y",
    "not-a-version",
    ">= 1.0.0, < 2.0.0",
];

const SCHEMES: &[(&str, VersionScheme)] = &[
    ("semver", VersionScheme::Semver),
    ("calver", VersionScheme::Calver),
    ("opaque", VersionScheme::Opaque),
];

fn metadata(scheme: VersionScheme, versions: Vec<String>) -> PackageMetadata {
    PackageMetadata {
        org: "acme".to_string(),
        name: "conformance".to_string(),
        vcs: Vcs::Git,
        repo_url: "https://github.com/acme/conformance".to_string(),
        description: None,
        latest: versions.last().cloned(),
        versions,
        version_scheme: scheme,
        tags: Vec::new(),
        mirrors: Vec::new(),
        signing_keys: Vec::new(),
    }
}

fn json_array(values: &[String]) -> String {
    let items: Vec<String> = values
        .iter()
        .map(|v| serde_json::to_string(v).expect("string"))
        .collect();
    format!("[{}]", items.join(", "))
}

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/cases");
    fs::create_dir_all(&dir).expect("cases dir");

    let mut rng = Lcg(0x5EED_1234);
    let mut resolution = Vec::new();
    let mut latest = Vec::new();

    for index in 0..400 {
        let (scheme_name, scheme) = rng.pick(SCHEMES);
        let count = rng.range(0, 6);
        let mut versions: Vec<String> = (0..count)
            .map(|_| (*rng.pick(VERSION_POOL)).to_string())
            .collect();
        versions.sort();
        versions.dedup();
        let requirement = *rng.pick(REQUIREMENT_POOL);
        let meta = metadata(*scheme, versions.clone());

        let expect = match resolve_version(&meta, requirement) {
            Ok(version) => format!(
                "{{ \"version\": {} }}",
                serde_json::to_string(version).unwrap()
            ),
            Err(error) => format!("{{ \"error\": \"{}\" }}", error.kind()),
        };
        resolution.push(format!(
            "    {{\n      \"name\": \"fuzz/{index}\",\n      \"scheme\": \"{scheme_name}\",\n      \"versions\": {},\n      \"requirement\": {},\n      \"expect\": {expect}\n    }}",
            json_array(&versions),
            serde_json::to_string(requirement).unwrap(),
        ));

        // Same shape, different operation: `latest_stable` has no requirement
        // and its own rules about prereleases and a stale `latest` field.
        if index % 2 == 0 {
            let expect = match latest_stable(&meta) {
                Some(version) => {
                    format!(
                        "{{ \"version\": {} }}",
                        serde_json::to_string(version).unwrap()
                    )
                }
                None => "{ \"version\": null }".to_string(),
            };
            latest.push(format!(
                "    {{\n      \"name\": \"fuzz/{index}\",\n      \"scheme\": \"{scheme_name}\",\n      \"versions\": {},\n      \"latest\": {},\n      \"expect\": {expect}\n    }}",
                json_array(&versions),
                meta.latest
                    .as_ref()
                    .map(|v| serde_json::to_string(v).unwrap())
                    .unwrap_or_else(|| "null".to_string()),
            ));
        }
    }

    let resolution_doc = format!(
        "{{\n  \"schema\": \"zed-lib/conformance/version-resolution/v1\",\n  \"description\": \"GENERATED by `cargo run --example generate_fuzz_corpus` — do not edit by hand. Pseudo-random cases answered by the Rust implementation, which every other slice must reproduce. Deterministic: regenerating rewrites this file byte for byte.\",\n  \"cases\": [\n{}\n  ]\n}}\n",
        resolution.join(",\n")
    );
    let latest_doc = format!(
        "{{\n  \"schema\": \"zed-lib/conformance/latest-stable/v1\",\n  \"description\": \"GENERATED by `cargo run --example generate_fuzz_corpus` — do not edit by hand. Pseudo-random `latest_stable` cases answered by the Rust implementation.\",\n  \"cases\": [\n{}\n  ]\n}}\n",
        latest.join(",\n")
    );

    fs::write(dir.join("fuzz-version-resolution.json"), resolution_doc).expect("write");
    fs::write(dir.join("fuzz-latest-stable.json"), latest_doc).expect("write");
    println!(
        "wrote {} resolution cases and {} latest-stable cases",
        resolution.len(),
        latest.len()
    );
}
