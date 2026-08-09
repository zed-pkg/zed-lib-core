use crate::OrmError;

pub(crate) const MAX_EMBEDDING_DIMENSIONS: usize = 8_192;

pub(crate) fn required_text(
    field: &str,
    value: &str,
    max_bytes: usize,
) -> Result<(), OrmError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > max_bytes {
        Err(OrmError::policy(format!(
            "{field} must contain between 1 and {max_bytes} UTF-8 bytes"
        )))
    } else {
        Ok(())
    }
}

#[cfg(feature = "read-write")]
pub(crate) fn optional_text(
    field: &str,
    value: Option<&str>,
    max_bytes: usize,
) -> Result<(), OrmError> {
    if let Some(value) = value {
        if value.len() > max_bytes {
            return Err(OrmError::policy(format!(
                "{field} may contain at most {max_bytes} UTF-8 bytes"
            )));
        }
    }
    Ok(())
}

#[cfg(feature = "read-write")]
pub(crate) fn one_of(field: &str, value: &str, allowed: &[&str]) -> Result<(), OrmError> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(OrmError::policy(format!(
            "{field} must be one of {}",
            allowed.join(", ")
        )))
    }
}

#[cfg(feature = "read-write")]
pub(crate) fn optional_one_of(
    field: &str,
    value: Option<&str>,
    allowed: &[&str],
) -> Result<(), OrmError> {
    match value {
        Some(value) => one_of(field, value, allowed),
        None => Ok(()),
    }
}

#[cfg(feature = "read-write")]
pub(crate) fn optional_nonnegative(field: &str, value: Option<i64>) -> Result<(), OrmError> {
    if value.is_some_and(|value| value < 0) {
        Err(OrmError::policy(format!("{field} cannot be negative")))
    } else {
        Ok(())
    }
}

#[cfg(feature = "read-write")]
pub(crate) fn optional_sha256(field: &str, value: Option<&str>) -> Result<(), OrmError> {
    if value.is_some_and(|value| !is_sha256(value)) {
        Err(OrmError::policy(format!(
            "{field} must be 64 lowercase hexadecimal characters"
        )))
    } else {
        Ok(())
    }
}

#[cfg(feature = "read-write")]
pub(crate) fn sha256(field: &str, value: &str) -> Result<(), OrmError> {
    optional_sha256(field, Some(value))
}

pub(crate) fn embedding_model(value: &str) -> Result<(), OrmError> {
    required_text("embedding model", value, 120)?;
    if value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'/' | b'-'))
    {
        Ok(())
    } else {
        Err(OrmError::policy(
            "embedding model contains unsupported characters",
        ))
    }
}

pub(crate) fn embedding(values: &[f32]) -> Result<(), OrmError> {
    if values.is_empty() || values.len() > MAX_EMBEDDING_DIMENSIONS {
        return Err(OrmError::policy(format!(
            "embedding dimensions must be between 1 and {MAX_EMBEDDING_DIMENSIONS}"
        )));
    }
    if values.iter().any(|value| !value.is_finite()) {
        return Err(OrmError::policy(
            "embedding values must all be finite",
        ));
    }
    let norm_squared = values
        .iter()
        .map(|value| f64::from(*value).powi(2))
        .sum::<f64>();
    if norm_squared == 0.0 {
        Err(OrmError::policy("embedding vector cannot be all zeros"))
    } else {
        Ok(())
    }
}

#[cfg(feature = "read-write")]
pub(crate) fn entity_type(value: &str) -> Result<(), OrmError> {
    one_of(
        "embedding entity type",
        value,
        &["org", "project", "package", "package_version"],
    )
}

#[cfg(feature = "read-write")]
fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "read-write")]
    #[test]
    fn hashes_are_exact_lowercase_sha256() {
        assert!(sha256("digest", &"a".repeat(64)).is_ok());
        assert!(sha256("digest", &"A".repeat(64)).is_err());
        assert!(sha256("digest", &"a".repeat(63)).is_err());
    }

    #[test]
    fn embeddings_are_bounded_finite_and_nonzero() {
        assert!(embedding(&[1.0, 0.5]).is_ok());
        assert!(embedding(&[]).is_err());
        assert!(embedding(&[0.0, 0.0]).is_err());
        assert!(embedding(&[f32::NAN]).is_err());
        assert!(embedding(&vec![1.0; MAX_EMBEDDING_DIMENSIONS + 1]).is_err());
    }

    #[test]
    fn model_tokens_match_the_shared_schema() {
        assert!(embedding_model("text-embedding/3:large").is_ok());
        assert!(embedding_model("contains spaces").is_err());
    }

    #[cfg(feature = "read-write")]
    #[test]
    fn entity_tokens_match_the_shared_schema() {
        assert!(entity_type("package_version").is_ok());
        assert!(entity_type("upload").is_err());
    }
}
