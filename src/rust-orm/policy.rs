//! The private→public promotion window.
//!
//! The database is the enforcement point: `zed_packages_visibility_guard`
//! rejects an out-of-window promotion no matter who issues it. This module is
//! the *pre-check* half — it lets a service refuse early and explain why,
//! instead of surfacing a raw Postgres exception to a user.
//!
//! The limits are read from the database functions
//! `zed_public_conversion_max_age_days()` and
//! `zed_public_conversion_max_downloads()` rather than hardcoded here, so
//! changing the policy stays a one-line contract change. A service that
//! hardcodes 10 and 50 will drift; one that calls [`VisibilityLimits::load`]
//! cannot.

use sea_orm::{ConnectionTrait, DatabaseConnection, Statement};

use crate::error::OrmError;

/// The promotion window, as the database currently defines it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibilityLimits {
    max_age_days: i32,
    max_downloads: i64,
}

impl VisibilityLimits {
    #[must_use]
    pub fn max_age_days(&self) -> i32 {
        self.max_age_days
    }

    #[must_use]
    pub fn max_downloads(&self) -> i64 {
        self.max_downloads
    }

    pub(crate) async fn load(connection: &DatabaseConnection) -> Result<Self, OrmError> {
        let statement = Statement::from_string(
            connection.get_database_backend(),
            "SELECT zed_public_conversion_max_age_days() AS max_age_days, \
             zed_public_conversion_max_downloads() AS max_downloads",
        );
        let row = connection
            .query_one(statement)
            .await
            .map_err(OrmError::from_db_err)?
            .ok_or_else(|| OrmError::policy("visibility limit query returned no row"))?;

        Ok(Self {
            max_age_days: row
                .try_get::<i32>("", "max_age_days")
                .map_err(OrmError::from_db_err)?,
            max_downloads: row
                .try_get::<i64>("", "max_downloads")
                .map_err(OrmError::from_db_err)?,
        })
    }

    /// Decide whether a package may still be promoted, given its age and
    /// download count.
    ///
    /// Boundaries match the trigger exactly: the checks are strictly greater
    /// than, so a package that is *exactly* at the limit still promotes.
    #[must_use]
    pub fn evaluate(&self, age_days: f64, download_count: i64) -> Option<PromotionRefusal> {
        if age_days > f64::from(self.max_age_days) {
            return Some(PromotionRefusal::TooOld {
                age_days,
                max_age_days: self.max_age_days,
            });
        }
        if download_count > self.max_downloads {
            return Some(PromotionRefusal::TooManyDownloads {
                download_count,
                max_downloads: self.max_downloads,
            });
        }
        None
    }
}

/// Why a package cannot be made public, phrased for a user rather than a log.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PromotionRefusal {
    TooOld {
        age_days: f64,
        max_age_days: i32,
    },
    TooManyDownloads {
        download_count: i64,
        max_downloads: i64,
    },
}

impl std::fmt::Display for PromotionRefusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooOld {
                age_days,
                max_age_days,
            } => write!(
                formatter,
                "this package has existed for {age_days:.0} days; \
                 a package can only be made public within its first {max_age_days} days"
            ),
            Self::TooManyDownloads {
                download_count,
                max_downloads,
            } => write!(
                formatter,
                "this package has {download_count} downloads; \
                 a package can only be made public with at most {max_downloads}"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> VisibilityLimits {
        VisibilityLimits {
            max_age_days: 10,
            max_downloads: 50,
        }
    }

    #[test]
    fn a_fresh_lightly_used_package_promotes() {
        assert!(limits().evaluate(0.0, 0).is_none());
        assert!(limits().evaluate(9.9, 49).is_none());
    }

    #[test]
    fn the_boundaries_themselves_still_promote() {
        // The trigger uses `>`, not `>=`. If these ever start refusing, the
        // pre-check has drifted from the database and will reject writes the
        // database would have accepted.
        assert!(limits().evaluate(10.0, 50).is_none());
    }

    #[test]
    fn just_past_either_boundary_is_refused() {
        assert!(matches!(
            limits().evaluate(10.1, 0),
            Some(PromotionRefusal::TooOld { .. })
        ));
        assert!(matches!(
            limits().evaluate(0.0, 51),
            Some(PromotionRefusal::TooManyDownloads { .. })
        ));
    }

    #[test]
    fn age_is_reported_before_downloads_when_both_fail() {
        // Matches the trigger's evaluation order, so the message a user sees is
        // the same whichever layer refuses first.
        assert!(matches!(
            limits().evaluate(30.0, 500),
            Some(PromotionRefusal::TooOld { .. })
        ));
    }
}
