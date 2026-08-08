use std::fmt;

/// Error type exposed by the ORM boundary.
///
/// Raw SeaORM/SQLx error types are deliberately converted to strings so the
/// public API does not make the persistence implementation part of every
/// consumer's contract.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OrmError {
    Database(String),
    PolicyViolation(String),
}

impl OrmError {
    pub(crate) fn database(error: impl fmt::Display) -> Self {
        Self::Database(error.to_string())
    }

    pub(crate) fn policy(message: impl Into<String>) -> Self {
        Self::PolicyViolation(message.into())
    }
}

impl fmt::Display for OrmError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(message) => write!(formatter, "database error: {message}"),
            Self::PolicyViolation(message) => write!(formatter, "database policy violation: {message}"),
        }
    }
}

impl std::error::Error for OrmError {}
