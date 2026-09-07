//! Explicit external-corpus conformance runner. It accepts no CLI options and
//! uses only the pinned checkout provisioned by public-contract-runtime.yml.
//! Kept in the existing server validation crate because it already owns the
//! serde_json dependency; no duplicate workspace or regenerated lock is needed.
use garde::Validate;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
use zed_validation::{PageQuery, ProblemDetails, RequestMeta};

fn checked<T>(value: &Value) -> Option<Value>
where
    T: DeserializeOwned + Serialize + Validate<Context = ()>,
{
    let parsed: T = serde_json::from_value(value.clone()).ok()?;
    parsed.validate().ok()?;
    serde_json::to_value(parsed).ok()
}

fn main() {
    assert_eq!(std::env::args_os().count(), 1, "no CLI options accepted");
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.deps/zed-interfaces/validation/compiler/cases.json");
    let corpus: Value = serde_json::from_str(
        &std::fs::read_to_string(path).expect("pinned production corpus checkout is mandatory"),
    )
    .unwrap();
    assert_eq!(corpus["schema"], "zed.public-validation-corpus/v1");
    let cases = corpus["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 34);
    let mut failures = Vec::new();
    for case in cases {
        let value = &case["value"];
        let parsed = match case["declaration"].as_str().unwrap() {
            "RequestMeta" => checked::<RequestMeta>(value),
            "PageQuery" => checked::<PageQuery>(value),
            "ProblemDetails" => checked::<ProblemDetails>(value),
            "PublicValidationContract" => {
                let mut matches: Vec<_> = [
                    checked::<RequestMeta>(value),
                    checked::<PageQuery>(value),
                    checked::<ProblemDetails>(value),
                ]
                .into_iter()
                .flatten()
                .collect();
                if matches.len() == 1 {
                    matches.pop()
                } else {
                    None
                }
            }
            other => panic!("unimplemented declaration: {other}"),
        };
        let expected = case["valid"].as_bool().unwrap();
        if parsed.is_some() != expected || (expected && parsed.as_ref() != Some(value)) {
            failures.push(format!(
                "{}/{}: expected valid={expected}, output={parsed:?}",
                case["declaration"], case["id"]
            ));
        }
    }
    for (value, valid) in [
        (json!({"requestId":"😀".repeat(128),"traceId":"t"}), true),
        (json!({"requestId":"😀".repeat(129),"traceId":"t"}), false),
        (json!({"requestId":"r","traceId":"t","locale":"😀"}), false),
    ] {
        if checked::<RequestMeta>(&value).is_some() != valid {
            failures.push(format!("Unicode length disagreement: expected valid={valid}"));
        }
    }
    for wire in [r#"{"limit":50.0}"#, r#"{"limit":5e1}"#] {
        let accepted = serde_json::from_str::<PageQuery>(wire)
            .map(|value| value.validate().is_ok())
            .unwrap_or(false);
        if !accepted {
            failures.push(format!("mathematical integer rejected: {wire}"));
        }
    }
    for wire in [r#"{"limit":1.5}"#, r#"{"limit":"50"}"#, r#"{"limit":null}"#] {
        if serde_json::from_str::<PageQuery>(wire).is_ok() {
            failures.push(format!("invalid integer payload accepted: {wire}"));
        }
    }
    assert!(
        failures.is_empty(),
        "runtime/schema disagreements:\n{}",
        failures.join("\n")
    );
    println!("34 production corpus cases and 8 Unicode/integer checks passed");
}
