use std::fmt;

/// SQLSTATE raised by `zed_packages_visibility_guard` when the package is older
/// than the promotion window.
pub const SQLSTATE_VISIBILITY_TOO_OLD: &str = "ZD001";

/// SQLSTATE raised by `zed_packages_visibility_guard` when the package has more
/// downloads than the promotion window allows.
pub const SQLSTATE_VISIBILITY_TOO_MANY_DOWNLOADS: &str = "ZD002";

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
    /// The package outlived the private→public promotion window. Callers should
    /// surface this as a 409, not a 500.
    VisibilityWindowExpired(String),
    /// The package passed the download ceiling for promotion to public.
    VisibilityDownloadLimitExceeded(String),
    NotFound(String),
}

impl OrmError {
    pub(crate) fn database(error: impl fmt::Display) -> Self {
        Self::Database(error.to_string())
    }

    pub(crate) fn policy(message: impl Into<String>) -> Self {
        Self::PolicyViolation(message.into())
    }

    // Only the write surface constructs this today; the read surface returns
    // Option instead of erroring on a miss.
    #[cfg_attr(not(feature = "read-write"), allow(dead_code))]
    pub(crate) fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }

    /// Whether this error is the caller's fault (a 4xx) rather than ours.
    ///
    /// The two visibility variants are the promotion rule firing as designed,
    /// so a service must not report them as internal failures.
    #[must_use]
    pub fn is_client_error(&self) -> bool {
        matches!(
            self,
            Self::VisibilityWindowExpired(_)
                | Self::VisibilityDownloadLimitExceeded(_)
                | Self::NotFound(_)
        )
    }

    /// Translate a SeaORM error, promoting the registry's dedicated SQLSTATEs
    /// to typed variants.
    ///
    /// Matching on the SQLSTATE rather than the message text is deliberate: the
    /// trigger's wording carries row ids and counts and is not a stable API.
    pub(crate) fn from_db_err(error: sea_orm::DbErr) -> Self {
        match sqlstate_of(&error).as_deref() {
            Some(SQLSTATE_VISIBILITY_TOO_OLD) => Self::VisibilityWindowExpired(error.to_string()),
            Some(SQLSTATE_VISIBILITY_TOO_MANY_DOWNLOADS) => {
                Self::VisibilityDownloadLimitExceeded(error.to_string())
            }
            _ => Self::database(error),
        }
    }
}

/// Pull the five-character SQLSTATE out of a SeaORM error, if the underlying
/// driver reported one.
fn sqlstate_of(error: &sea_orm::DbErr) -> Option<String> {
    use sea_orm::{DbErr, RuntimeErr};

    let runtime = match error {
        DbErr::Exec(runtime) | DbErr::Query(runtime) | DbErr::Conn(runtime) => runtime,
        _ => return None,
    };

    match runtime {
        RuntimeErr::SqlxError(sea_orm::sqlx::Error::Database(database_error)) => {
            database_error.code().map(|code| code.into_owned())
        }
        _ => None,
    }
}

impl fmt::Display for OrmError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(message) => write!(formatter, "database error: {message}"),
            Self::PolicyViolation(message) => {
                write!(formatter, "database policy violation: {message}")
            }
            Self::VisibilityWindowExpired(message) => {
                write!(formatter, "package is too old to be made public: {message}")
            }
            Self::VisibilityDownloadLimitExceeded(message) => write!(
                formatter,
                "package has too many downloads to be made public: {message}"
            ),
            Self::NotFound(message) => write!(formatter, "not found: {message}"),
        }
    }
}

impl std::error::Error for OrmError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visibility_variants_are_client_errors() {
        assert!(OrmError::VisibilityWindowExpired("x".into()).is_client_error());
        assert!(OrmError::VisibilityDownloadLimitExceeded("x".into()).is_client_error());
        assert!(OrmError::NotFound("x".into()).is_client_error());
    }

    #[test]
    fn infrastructure_failures_are_not_client_errors() {
        assert!(!OrmError::Database("connection reset".into()).is_client_error());
        assert!(!OrmError::PolicyViolation("not read-only".into()).is_client_error());
    }
}
