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

#[test]
fn matches_the_admitted_production_corpus_without_null_insertion() {
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
                if matches.len() == 1 { matches.pop() } else { None }
            }
            other => panic!("unimplemented declaration: {other}"),
        };
        let expected = case["valid"].as_bool().unwrap();
        if parsed.is_some() != expected || (expected && parsed.as_ref() != Some(value)) {
            failures.push(format!("{}/{}: expected valid={expected}, output={parsed:?}", case["declaration"], case["id"]));
        }
    }
    assert!(failures.is_empty(), "runtime/schema disagreements:\n{}", failures.join("\n"));
}

#[test]
fn unicode_lengths_use_code_points() {
    assert!(checked::<RequestMeta>(&json!({"requestId":"😀".repeat(128),"traceId":"t"})).is_some());
    assert!(checked::<RequestMeta>(&json!({"requestId":"😀".repeat(129),"traceId":"t"})).is_none());
    assert!(checked::<RequestMeta>(&json!({"requestId":"r","traceId":"t","locale":"😀"})).is_none());
}

#[test]
fn integer_json_spellings_are_not_string_coercion() {
    for wire in [r#"{"limit":50.0}"#, r#"{"limit":5e1}"#] {
        let parsed: PageQuery = serde_json::from_str(wire).expect("JSON mathematical integers must decode");
        assert!(parsed.validate().is_ok());
    }
    for wire in [r#"{"limit":1.5}"#, r#"{"limit":"50"}"#, r#"{"limit":null}"#] {
        assert!(serde_json::from_str::<PageQuery>(wire).is_err());
    }
}
