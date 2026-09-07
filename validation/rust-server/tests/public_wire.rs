use garde::Validate;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
use zed_validation::{PageQuery, ProblemDetails, RequestMeta};

fn checked<T>(value: Value) -> Result<Value, String>
where
    T: DeserializeOwned + Serialize + Validate<Context = ()>,
{
    let parsed: T = serde_json::from_value(value).map_err(|error| error.to_string())?;
    parsed.validate().map_err(|error| error.to_string())?;
    serde_json::to_value(parsed).map_err(|error| error.to_string())
}

#[test]
fn absent_optional_properties_stay_absent_on_egress() {
    let request = json!({"requestId":"r","traceId":"t"});
    assert_eq!(checked::<RequestMeta>(request.clone()).unwrap(), request);
    let page = json!({"limit":50});
    assert_eq!(checked::<PageQuery>(page.clone()).unwrap(), page);
    let problem = json!({"type":"error","title":"Error","status":500,"requestId":"r"});
    assert_eq!(checked::<ProblemDetails>(problem.clone()).unwrap(), problem);
}

#[test]
fn explicit_null_is_not_a_missing_optional_string() {
    let request = json!({"requestId":"r","traceId":"t","locale":null});
    assert!(checked::<RequestMeta>(request).is_err());
    assert!(checked::<PageQuery>(json!({"limit":50,"cursor":null})).is_err());
    let problem = json!({
        "type":"error", "title":"Error", "status":500, "requestId":"r", "detail":null
    });
    assert!(checked::<ProblemDetails>(problem).is_err());
}

#[test]
fn present_optional_properties_survive_roundtrip() {
    let request = json!({"requestId":"r","traceId":"t","locale":"es-PE"});
    assert_eq!(checked::<RequestMeta>(request.clone()).unwrap(), request);
    let page = json!({"limit":50,"cursor":"next"});
    assert_eq!(checked::<PageQuery>(page.clone()).unwrap(), page);
    let problem = json!({
        "type":"error", "title":"Error", "status":500, "requestId":"r", "detail":""
    });
    assert_eq!(checked::<ProblemDetails>(problem.clone()).unwrap(), problem);
}

#[test]
fn string_length_counts_unicode_scalars_not_bytes_or_graphemes() {
    for (length, valid) in [(128, true), (129, false)] {
        let request = json!({"requestId":"😀".repeat(length),"traceId":"t"});
        assert_eq!(checked::<RequestMeta>(request).is_ok(), valid);
    }
    for (locale, valid) in [("😀", false), ("e\u{0301}", true)] {
        let request = json!({"requestId":"r","traceId":"t","locale":locale});
        assert_eq!(checked::<RequestMeta>(request).is_ok(), valid);
    }
    for (length, valid) in [(512, true), (513, false)] {
        let page = json!({"limit":50,"cursor":"😀".repeat(length)});
        assert_eq!(checked::<PageQuery>(page).is_ok(), valid);
    }
}

#[test]
fn integral_decimal_and_exponent_spellings_decode_without_string_coercion() {
    for wire in [r#"{"limit":50}"#, r#"{"limit":50.0}"#, r#"{"limit":5e1}"#] {
        let parsed: PageQuery = serde_json::from_str(wire).unwrap();
        assert!(parsed.validate().is_ok());
        assert_eq!(parsed.limit, 50);
    }
    let wire = r#"{"type":"e","title":"E","status":5e2,"requestId":"r"}"#;
    let parsed: ProblemDetails = serde_json::from_str(wire).unwrap();
    assert!(parsed.validate().is_ok());
    assert_eq!(parsed.status, 500);
}

#[test]
fn missing_and_invalid_numeric_values_fail() {
    for value in [
        json!({}),
        json!({"limit":null}),
        json!({"limit":"50"}),
        json!({"limit":true}),
        json!({"limit":1.5}),
        json!({"limit":-1}),
        json!({"limit":65536}),
        json!({"limit":0}),
        json!({"limit":101}),
    ] {
        assert!(
            checked::<PageQuery>(value.clone()).is_err(),
            "accepted {value}"
        );
    }
}

#[test]
fn unknown_keys_remain_rejected() {
    let request = json!({"requestId":"r","traceId":"t","secret":true});
    assert!(checked::<RequestMeta>(request).is_err());
    assert!(checked::<PageQuery>(json!({"limit":50,"admin":true})).is_err());
    let wire = r#"{"requestId":"r","traceId":"t","__proto__":{}}"#;
    let value: Value = serde_json::from_str(wire).unwrap();
    assert!(checked::<RequestMeta>(value).is_err());
}

#[test]
fn whitespace_is_wire_data_not_implicit_normalization() {
    let request = json!({"requestId":" r ","traceId":" t ","locale":"  "});
    assert_eq!(checked::<RequestMeta>(request.clone()).unwrap(), request);
    let page = json!({"limit":50,"cursor":" "});
    assert_eq!(checked::<PageQuery>(page.clone()).unwrap(), page);
}
