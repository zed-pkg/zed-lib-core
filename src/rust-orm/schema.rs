//! Registry schema naming helpers.
//!
//! The existing deployed registry tables live in `public`. The migration owner
//! therefore preserves that namespace during the first ownership cutover. A
//! future expand/backfill/contract migration may move the complete graph into a
//! dedicated schema without making every service invent its own transition.

/// Current schema containing the deployed registry tables.
pub const REGISTRY_SCHEMA: &str = "public";

/// Return a safely qualified registry table name.
pub fn qualified(table: &str) -> Result<String, &'static str> {
    if table.is_empty()
        || table.trim() != table
        || !table
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err("invalid registry table name");
    }
    Ok(format!("{REGISTRY_SCHEMA}.{table}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qualifies_only_canonical_identifiers() {
        assert_eq!(qualified("projects").unwrap(), "public.projects");
        for invalid in ["", " projects", "Projects", "projects;drop", "project-name"] {
            assert!(qualified(invalid).is_err(), "{invalid:?}");
        }
    }
}
