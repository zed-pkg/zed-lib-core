#![forbid(unsafe_code)]

use garde::Validate;
use serde::{Deserialize, Serialize};

mod wire;

pub const VALIDATION_CONTRACT_VERSION: &str = "ores.validation.v1";

#[derive(Clone, Debug, Deserialize, Serialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestMeta {
    #[garde(length(chars, min = 1, max = 128))]
    pub request_id: String,
    #[garde(length(chars, min = 1, max = 128))]
    pub trace_id: String,
    #[serde(
        default,
        deserialize_with = "wire::optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    #[garde(length(chars, min = 2, max = 64))]
    pub locale: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PageQuery {
    #[serde(deserialize_with = "wire::integer_u16")]
    #[garde(range(min = 1, max = 100))]
    pub limit: u16,
    #[serde(
        default,
        deserialize_with = "wire::optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    #[garde(length(chars, min = 1, max = 512))]
    pub cursor: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProblemDetails {
    #[garde(length(chars, min = 1, max = 512))]
    pub r#type: String,
    #[garde(length(chars, min = 1, max = 256))]
    pub title: String,
    #[serde(deserialize_with = "wire::integer_u16")]
    #[garde(range(min = 400, max = 599))]
    pub status: u16,
    #[serde(
        default,
        deserialize_with = "wire::optional_string",
        skip_serializing_if = "Option::is_none"
    )]
    #[garde(length(chars, max = 4096))]
    pub detail: Option<String>,
    #[garde(length(chars, min = 1, max = 128))]
    pub request_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_request_id() {
        let value = RequestMeta {
            request_id: String::new(),
            trace_id: "trace-1".into(),
            locale: None,
        };
        assert!(value.validate().is_err());
    }
}
