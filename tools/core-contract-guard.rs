//! Fail-closed boundary and parity guard for `*-lib-core` and `*-orm-core`.
//! Inputs are independently generated, canonical NDJSON evidence files.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

const PAIRS: &[(&str, &str, &str)] = &[
    ("interfaces", "artifacts/interfaces-ir/typespec.ndjson", "artifacts/interfaces-ir/json-schema.ndjson"),
    ("contract", "artifacts/contract-ir/typespec.ndjson", "artifacts/contract-ir/json-schema.ndjson"),
    ("persistence", "artifacts/persistence-ir/typespec.ndjson", "artifacts/persistence-ir/json-schema.ndjson"),
    ("sql_catalog", "artifacts/sql-catalog/typespec.ndjson", "artifacts/sql-catalog/json-schema.ndjson"),
    ("orm", "artifacts/orm-ir/typespec.ndjson", "artifacts/orm-ir/json-schema.ndjson"),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Kind { LibCore, OrmCore }

impl Kind {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "lib-core" => Ok(Self::LibCore),
            "orm-core" => Ok(Self::OrmCore),
            _ => Err(format!("unsupported kind {value:?}")),
        }
    }
    fn name(self) -> &'static str {
        match self { Self::LibCore => "lib-core", Self::OrmCore => "orm-core" }
    }
}

struct Args { root: PathBuf, kind: Kind, strict: bool, finalize: bool }

fn main() {
    if let Err(error) = run() {
        eprintln!("core-contract-guard: {error}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = args()?;
    let config = read_kv(&args.root.join("core-boundary.toml"))?;
    let configured = Kind::parse(required(&config, "kind")?)?;
    if configured != args.kind {
        return Err(format!("configured kind {} != CLI kind {}", configured.name(), args.kind.name()));
    }
    validate_coordinates(&config, args.kind)?;
    validate_layout(&args.root, args.kind, args.strict || args.finalize)?;
    validate_zed(&args.root, &config, args.kind, args.strict || args.finalize)?;
    validate_source_lock(&args.root, args.strict || args.finalize)?;
    let evidence = compare_evidence(&args.root, args.strict || args.finalize)?;
    if args.finalize {
        write_agreement(&args.root, args.kind, &config, &evidence)?;
        println!("core-contract-guard: equivalent evidence finalized");
    } else if args.strict {
        verify_agreement(&args.root, args.kind, &config, &evidence)?;
        println!("core-contract-guard: release evidence verified");
    } else if evidence.is_empty() {
        println!("core-contract-guard: bootstrap boundary valid; release remains blocked until parity evidence is complete");
    } else {
        println!("core-contract-guard: boundary and complete parity evidence valid");
    }
    Ok(())
}

fn args() -> Result<Args, String> {
    let mut root = PathBuf::from(".");
    let mut kind = None;
    let mut strict = false;
    let mut finalize = false;
    let mut values = env::args().skip(1);
    while let Some(value) = values.next() {
        match value.as_str() {
            "--root" => root = PathBuf::from(values.next().ok_or("--root needs a value")?),
            "--kind" => kind = Some(Kind::parse(&values.next().ok_or("--kind needs a value")?)?),
            "--require-evidence" => strict = true,
            "--finalize" => finalize = true,
            "--help" | "-h" => {
                println!("usage: core-contract-guard --kind <lib-core|orm-core> [--root PATH] [--require-evidence] [--finalize]");
                process::exit(0);
            }
            _ => return Err(format!("unknown argument {value:?}")),
        }
    }
    Ok(Args { root, kind: kind.ok_or("--kind is required")?, strict, finalize })
}

fn read_kv(path: &Path) -> Result<BTreeMap<String, String>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut out = BTreeMap::new();
    for (number, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('[') { continue; }
        let (key, value) = line.split_once('=').ok_or_else(|| format!("{}:{}: expected key = value", path.display(), number + 1))?;
        let key = key.trim().to_owned();
        let value = value.trim().trim_matches('"').to_owned();
        if out.insert(key.clone(), value).is_some() {
            return Err(format!("{}:{}: duplicate key {key}", path.display(), number + 1));
        }
    }
    Ok(out)
}

fn required<'a>(map: &'a BTreeMap<String, String>, key: &str) -> Result<&'a str, String> {
    map.get(key).map(String::as_str).filter(|v| !v.is_empty()).ok_or_else(|| format!("missing non-empty {key}"))
}

fn split_coordinate<'a>(key: &str, value: &'a str) -> Result<(&'a str, &'a str), String> {
    let (org, repo) = value.split_once('/').ok_or_else(|| format!("{key} must be org/repository"))?;
    if org.is_empty() || repo.is_empty() || repo.contains('/') { return Err(format!("{key} must be org/repository")); }
    Ok((org, repo))
}

fn validate_coordinates(config: &BTreeMap<String, String>, kind: Kind) -> Result<(), String> {
    let repository = required(config, "repository")?;
    let interfaces = required(config, "interfaces_repository")?;
    let lib_core = required(config, "lib_core_repository")?;
    let orm_core = required(config, "orm_core_repository")?;
    let (org, _) = split_coordinate("repository", repository)?;
    for (key, value) in [("interfaces_repository", interfaces), ("lib_core_repository", lib_core), ("orm_core_repository", orm_core)] {
        let (peer_org, _) = split_coordinate(key, value)?;
        if peer_org != org { return Err(format!("{key} must remain in organization {org}")); }
    }
    let expected = if kind == Kind::LibCore { lib_core } else { orm_core };
    if repository != expected { return Err(format!("repository must equal {expected}")); }
    let env_contract = required(config, "env_contract")?;
    if env_contract != "inline" {
        let (env_org, env_repo) = split_coordinate("env_contract", env_contract)?;
        if env_org != org || !env_repo.ends_with("-env") { return Err("env_contract must be inline or an org-local *-env repository".into()); }
    }
    Ok(())
}

fn validate_layout(root: &Path, kind: Kind, strict: bool) -> Result<(), String> {
    match kind {
        Kind::LibCore => {
            for dir in ["client", "server", "edge", "isomorph"] {
                if !root.join(dir).is_dir() { return Err(format!("lib-core requires {dir}/")); }
            }
            for path in ["orm", "generated/orm", "src/orm", "src/rust-orm"] {
                if root.join(path).exists() {
                    if strict { return Err(format!("{path} is executable ORM material and belongs only in orm-core")); }
                    eprintln!("core-contract-guard: bootstrap blocker: migrate and remove {path} before release");
                }
            }
        }
        Kind::OrmCore => {
            if !root.join("backend").is_dir() { return Err("orm-core requires backend/".into()); }
            if !root.join("PRIVATE_BACKEND_ONLY.md").is_file() { return Err("orm-core requires PRIVATE_BACKEND_ONLY.md".into()); }
            for dir in ["client", "edge", "isomorph"] {
                if root.join(dir).exists() { return Err(format!("backend-only orm-core may not expose {dir}/")); }
            }
        }
    }
    Ok(())
}

fn validate_zed(root: &Path, config: &BTreeMap<String, String>, kind: Kind, strict: bool) -> Result<(), String> {
    let path = root.join(".zpkg.toml");
    let text = fs::read_to_string(&path).map_err(|e| format!("{} is required: {e}", path.display()))?;
    for marker in ["[package]", "[package.repository]", "[install]", ".vendor/.zed"] {
        if !text.contains(marker) { return Err(format!("{} must contain {marker:?}", path.display())); }
    }
    if text.contains("k8s-libs-and-shared-defs") {
        return Err("central shared-definitions must not be product schema/ORM authority".into());
    }
    if strict {
        for coordinate in [required(config, "interfaces_repository")?, if kind == Kind::OrmCore { required(config, "lib_core_repository")? } else { "" }] {
            if !coordinate.is_empty() && !text.contains(coordinate) { return Err(format!("{} must pin {coordinate}", path.display())); }
        }
        for excluded in [".env", "env/dec", ".vendor/.zed"] {
            if !text.contains(excluded) { return Err(format!("{} must exclude {excluded}", path.display())); }
        }
    }
    Ok(())
}

fn validate_source_lock(root: &Path, strict: bool) -> Result<(), String> {
    let path = root.join("contracts/source-lock.toml");
    let values = read_kv(&path)?;
    for key in ["typespec_commit", "typespec_sha256", "json_schema_commit", "json_schema_sha256", "comparator_version"] {
        let value = required(&values, key)?;
        if strict && matches!(value, "UNPINNED" | "PENDING" | "BOOTSTRAP") {
            return Err(format!("{} contains bootstrap placeholder for {key}", path.display()));
        }
    }
    Ok(())
}

fn compare_evidence(root: &Path, strict: bool) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    let mut missing = Vec::new();
    for &(name, left_name, right_name) in PAIRS {
        let left = root.join(left_name);
        let right = root.join(right_name);
        match (left.exists(), right.exists()) {
            (false, false) => missing.push(name),
            (true, false) | (false, true) => return Err(format!("{name} evidence is one-sided")),
            (true, true) => {
                let left = canonical_ndjson(&left)?;
                let right = canonical_ndjson(&right)?;
                if left != right { return Err(format!("{name} differs between TypeSpec and JSON Schema lanes")); }
                out.insert(name.to_owned(), hex_sha256(&left));
            }
        }
    }
    if strict && !missing.is_empty() { return Err(format!("release evidence missing: {}", missing.join(", "))); }
    if !strict && !out.is_empty() && !missing.is_empty() { return Err(format!("partial evidence is forbidden; missing: {}", missing.join(", "))); }
    Ok(out)
}

fn canonical_ndjson(path: &Path) -> Result<Vec<u8>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut lines = Vec::new();
    let mut seen = BTreeSet::new();
    for (number, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() { continue; }
        if !line.starts_with('{') || !line.ends_with('}') { return Err(format!("{}:{} is not canonical NDJSON", path.display(), number + 1)); }
        if !seen.insert(line.to_owned()) { return Err(format!("{}:{} duplicates a record", path.display(), number + 1)); }
        lines.push(line.to_owned());
    }
    lines.sort();
    let mut bytes = lines.join("\n").into_bytes();
    if !bytes.is_empty() { bytes.push(b'\n'); }
    Ok(bytes)
}

fn write_agreement(root: &Path, kind: Kind, config: &BTreeMap<String, String>, evidence: &BTreeMap<String, String>) -> Result<(), String> {
    if evidence.len() != PAIRS.len() { return Err("cannot finalize incomplete evidence".into()); }
    let mut text = format!("format = \"ores.core-agreement/v1\"\nstatus = \"equivalent\"\nkind = \"{}\"\nrepository = \"{}\"\n", kind.name(), required(config, "repository")?);
    for (name, digest) in evidence { text.push_str(&format!("{name}_sha256 = \"{digest}\"\n")); }
    let path = root.join("artifacts/agreement.lock");
    if let Some(parent) = path.parent() { fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?; }
    fs::write(&path, text).map_err(|e| format!("write {}: {e}", path.display()))
}

fn verify_agreement(root: &Path, kind: Kind, config: &BTreeMap<String, String>, evidence: &BTreeMap<String, String>) -> Result<(), String> {
    let path = root.join("artifacts/agreement.lock");
    let lock = read_kv(&path)?;
    for (key, expected) in [("format", "ores.core-agreement/v1"), ("status", "equivalent"), ("kind", kind.name()), ("repository", required(config, "repository")?)] {
        if required(&lock, key)? != expected { return Err(format!("{} has stale {key}", path.display())); }
    }
    for (name, digest) in evidence {
        let key = format!("{name}_sha256");
        if required(&lock, &key)? != digest { return Err(format!("{} has stale {key}", path.display())); }
    }
    Ok(())
}

fn hex_sha256(input: &[u8]) -> String {
    sha256(input).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn sha256(input: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5,
        0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,
        0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc,0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,
        0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7,0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967,
        0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,
        0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3,0xd192e819,0xd6990624,0xf40e3585,0x106aa070,
        0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3,
        0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2,
    ];
    let mut h = [0x6a09e667u32,0xbb67ae85,0x3c6ef372,0xa54ff53a,0x510e527f,0x9b05688c,0x1f83d9ab,0x5be0cd19];
    let mut data = input.to_vec();
    let bit_len = (data.len() as u64) * 8;
    data.push(0x80);
    while data.len() % 64 != 56 { data.push(0); }
    data.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in data.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in chunk.chunks_exact(4).take(16).enumerate() { w[i] = u32::from_be_bytes([word[0],word[1],word[2],word[3]]); }
        for i in 16..64 {
            let s0 = w[i-15].rotate_right(7) ^ w[i-15].rotate_right(18) ^ (w[i-15] >> 3);
            let s1 = w[i-2].rotate_right(17) ^ w[i-2].rotate_right(19) ^ (w[i-2] >> 10);
            w[i] = w[i-16].wrapping_add(s0).wrapping_add(w[i-7]).wrapping_add(s1);
        }
        let (mut a,mut b,mut c,mut d,mut e,mut f,mut g,mut q) = (h[0],h[1],h[2],h[3],h[4],h[5],h[6],h[7]);
        for i in 0..64 {
            let t1 = q.wrapping_add(e.rotate_right(6)^e.rotate_right(11)^e.rotate_right(25)).wrapping_add((e&f)^(!e&g)).wrapping_add(K[i]).wrapping_add(w[i]);
            let t2 = (a.rotate_right(2)^a.rotate_right(13)^a.rotate_right(22)).wrapping_add((a&b)^(a&c)^(b&c));
            q=g; g=f; f=e; e=d.wrapping_add(t1); d=c; c=b; b=a; a=t1.wrapping_add(t2);
        }
        for (slot, value) in h.iter_mut().zip([a,b,c,d,e,f,g,q]) { *slot = slot.wrapping_add(value); }
    }
    let mut out = [0u8; 32];
    for (i, value) in h.iter().enumerate() { out[i*4..i*4+4].copy_from_slice(&value.to_be_bytes()); }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sha_vector() { assert_eq!(hex_sha256(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"); }
    #[test]
    fn kind_is_closed() { assert!(Kind::parse("clients").is_err()); }
}
