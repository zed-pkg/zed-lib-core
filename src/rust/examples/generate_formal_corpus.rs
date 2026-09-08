//! Project completed Quint ITF states into the shared production-test corpus.
//!
//! No resolver implementation is imported: expected answers come exclusively
//! from the model's transitions. Generate the pinned trace as documented in
//! conformance/README.md, then run `cargo run --locked -p zed-lib --example
//! generate_formal_corpus`. An incomplete or inconsistent trace fails closed.

use std::collections::{BTreeMap, btree_map::Entry};
use std::fs;
use std::path::Path;

use serde::Deserialize;
use serde_json::{Value, json};

const CASE_COUNT: u32 = 17;

#[derive(Deserialize)]
struct Trace {
    #[serde(rename = "#meta")]
    meta: Meta,
    vars: Vec<String>,
    states: Vec<Snapshot>,
}

#[derive(Deserialize)]
struct Meta {
    format: String,
    source: String,
    status: String,
}

#[derive(Deserialize)]
struct Snapshot {
    s: State,
}

#[derive(Deserialize)]
struct Integer {
    #[serde(rename = "#bigint")]
    value: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Mode {
    Resolve,
    Latest,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Pending,
    Resolved,
    NoVersions,
    InvalidRequirement,
    Unsatisfied,
}

#[derive(Deserialize)]
struct Input {
    name: String,
    scheme: String,
    mode: Mode,
    versions: Vec<String>,
    requirement: String,
    latest: String,
}

#[derive(Deserialize)]
struct State {
    case_id: Integer,
    done: bool,
    replaying: bool,
    input: Input,
    kind: Kind,
    version: String,
}

fn project(raw: &str) -> Result<(Value, Value), String> {
    let trace: Trace = serde_json::from_str(raw).map_err(|error| error.to_string())?;
    if trace.meta.format != "ITF"
        || trace.meta.status != "ok"
        || trace.meta.source != "formal/dependency_resolution.qnt"
        || trace.vars != ["s"]
    {
        return Err("expected a successful dependency-resolution ITF trace".into());
    }
    let mut completed = BTreeMap::new();
    for snapshot in trace.states {
        let state = snapshot.s;
        if !state.done {
            continue;
        }
        let id: u32 = state.case_id.value.parse().map_err(|_| "invalid case id")?;
        if !(1..=CASE_COUNT).contains(&id)
            || state.input.name.is_empty()
            || !matches!(state.input.scheme.as_str(), "semver" | "calver" | "opaque")
        {
            return Err("unknown or unnamed model fixture".into());
        }
        let expect = match (&state.input.mode, state.kind, state.version.as_str()) {
            (Mode::Latest, Kind::Resolved, "") => json!({"version": null}),
            (_, Kind::Resolved, version) if !version.is_empty() => {
                if !state
                    .input
                    .versions
                    .iter()
                    .any(|candidate| candidate == version)
                {
                    return Err(format!("case {id}: result is not published"));
                }
                json!({"version": version})
            }
            (Mode::Resolve, Kind::NoVersions, "") => json!({"error": "no_versions"}),
            (Mode::Resolve, Kind::InvalidRequirement, "") => {
                json!({"error": "invalid_requirement"})
            }
            (Mode::Resolve, Kind::Unsatisfied, "") => json!({"error": "unsatisfied"}),
            _ => return Err(format!("case {id}: inconsistent completed outcome")),
        };
        let mut case = json!({
            "name": format!("formal/{id}/{}", state.input.name),
            "scheme": state.input.scheme,
            "versions": state.input.versions,
            "expect": expect,
        });
        match state.input.mode {
            Mode::Resolve if !state.input.requirement.is_empty() => {
                case["requirement"] = json!(state.input.requirement);
            }
            Mode::Resolve => return Err(format!("case {id}: missing requirement")),
            Mode::Latest => {
                case["latest"] = if state.input.latest.is_empty() {
                    Value::Null
                } else {
                    json!(state.input.latest)
                };
            }
        }
        match completed.entry((id, state.replaying)) {
            Entry::Vacant(entry) => {
                entry.insert(case);
            }
            Entry::Occupied(entry) if entry.get() == &case => {}
            Entry::Occupied(_) => return Err(format!("case {id}: conflicting completed states")),
        }
    }
    let mut resolution = Vec::new();
    let mut latest = Vec::new();
    for id in 1..=CASE_COUNT {
        let first = completed
            .get(&(id, false))
            .ok_or_else(|| format!("missing case {id}"))?;
        let replay = completed
            .get(&(id, true))
            .ok_or_else(|| format!("missing replay {id}"))?;
        if first != replay {
            return Err(format!("case {id}: replay changed the input or result"));
        }
        for (label, source) in [("first", first), ("replay", replay)] {
            let mut case = source.clone();
            case["name"] = json!(format!("{}/{label}", source["name"].as_str().unwrap()));
            if case.get("requirement").is_some() {
                resolution.push(case);
            } else {
                latest.push(case);
            }
        }
    }
    let document = |kind: &str, cases: Vec<Value>| {
        json!({
            "schema": format!("zed-lib/conformance/{kind}/v1"),
            "description": "GENERATED from formal/dependency_resolution.qnt by the pinned Quint trace driver and generate_formal_corpus. Expected answers come from model transitions, not a production resolver. Do not edit by hand.",
            "cases": cases,
        })
    };
    Ok((
        document("version-resolution", resolution),
        document("latest-stable", latest),
    ))
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let raw =
        fs::read_to_string(root.join(".formal-artifacts/dependency-resolution/trace.itf.json"))
            .expect("generate the Quint trace first; see conformance/README.md");
    let (resolution, latest) = project(&raw).expect("complete, consistent model trace");
    for (file, document) in [
        ("formal-version-resolution.json", resolution),
        ("formal-latest-stable.json", latest),
    ] {
        let output = serde_json::to_string_pretty(&document).expect("JSON document") + "\n";
        fs::write(root.join("conformance/cases").join(file), output).expect("write formal corpus");
        println!(
            "{file}: {} model-derived cases",
            document["cases"].as_array().unwrap().len()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trace() -> Value {
        let states: Vec<_> = (1..=CASE_COUNT)
            .flat_map(|id| {
                [false, true].map(|replaying| json!({
            "s": {
                "case_id": {"#bigint": id.to_string()}, "done": true, "replaying": replaying,
                "kind": "resolved", "version": "1.0.0",
                "input": {
                    "name": "fixture", "scheme": "semver", "mode": "resolve",
                    "versions": ["1.0.0"], "requirement": "=1.0.0", "latest": "",
                },
            },
        }))
            })
            .collect();
        json!({"#meta": {"format": "ITF", "source": "formal/dependency_resolution.qnt", "status": "ok"}, "vars": ["s"], "states": states})
    }

    #[test]
    fn accepts_complete_trace_and_identical_stutters_deterministically() {
        let mut trace = trace();
        let expected = project(&trace.to_string()).unwrap();
        let duplicate = trace["states"][0].clone();
        trace["states"].as_array_mut().unwrap().push(duplicate);
        assert_eq!(project(&trace.to_string()).unwrap(), expected);
        assert_eq!(expected.0["cases"].as_array().unwrap().len(), 34);
    }

    #[test]
    fn rejects_incomplete_or_failed_traces() {
        let mut incomplete = trace();
        incomplete["states"].as_array_mut().unwrap().pop();
        assert!(
            project(&incomplete.to_string())
                .unwrap_err()
                .contains("missing replay")
        );
        let mut failed = trace();
        failed["#meta"]["status"] = json!("violation");
        assert!(project(&failed.to_string()).is_err());
    }

    #[test]
    fn rejects_conflicts_unknown_kinds_and_inconsistent_results() {
        for bad in ["pending", "unknown", "no_versions"] {
            let mut trace = trace();
            trace["states"][0]["s"]["kind"] = json!(bad);
            assert!(project(&trace.to_string()).is_err());
        }
        let mut conflict = trace();
        let mut duplicate = conflict["states"][0].clone();
        duplicate["s"]["input"]["name"] = json!("conflict");
        conflict["states"].as_array_mut().unwrap().push(duplicate);
        assert!(
            project(&conflict.to_string())
                .unwrap_err()
                .contains("conflicting")
        );
        let mut replay = trace();
        replay["states"][1]["s"]["input"]["name"] = json!("changed");
        assert!(
            project(&replay.to_string())
                .unwrap_err()
                .contains("replay changed")
        );
        let mut unpublished = trace();
        unpublished["states"][0]["s"]["version"] = json!("9.0.0");
        assert!(
            project(&unpublished.to_string())
                .unwrap_err()
                .contains("not published")
        );
    }
}
