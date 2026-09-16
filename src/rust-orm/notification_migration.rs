//! Forward-only migration for Zed package email notification state.
//!
//! Kept separate from the historical registry migration series so the feature
//! can ship without replaying or mutating the established registry ledger.
//! The API server's discrete `migrate` command invokes this after the canonical
//! registry migration batch.

use sea_orm::{ConnectionTrait, Statement, TransactionTrait};

use crate::{connection::WriteContext, error::OrmError};

const EMAIL_SQL: &str = include_str!("sql/2026-09-16-package-email-notifications.sql");
const EVENT_SQL: &str = include_str!("sql/2026-09-16-package-notification-events.sql");
const VERSION: &str = "package-email-notifications@2026-09-16-v1";
const MIGRATION_LOCK_KEY: i64 = 913_447_316;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotificationMigrationReport {
    pub version: &'static str,
    pub applied: bool,
}

pub const fn notification_migration_version() -> &'static str {
    VERSION
}

pub async fn migrate_notifications(
    context: &WriteContext,
) -> Result<NotificationMigrationReport, OrmError> {
    let transaction = context
        .connection()
        .begin()
        .await
        .map_err(OrmError::from_db_err)?;

    transaction
        .execute_unprepared(&format!(
            "SELECT pg_advisory_xact_lock({MIGRATION_LOCK_KEY}); \
             CREATE TABLE IF NOT EXISTS zed_schema_migrations (\
               version TEXT PRIMARY KEY, \
               applied_at TIMESTAMPTZ NOT NULL DEFAULT now()\
             );"
        ))
        .await
        .map_err(OrmError::from_db_err)?;

    let already_applied = transaction
        .query_one(Statement::from_sql_and_values(
            transaction.get_database_backend(),
            "SELECT version FROM zed_schema_migrations WHERE version = $1",
            [VERSION.into()],
        ))
        .await
        .map_err(OrmError::from_db_err)?
        .is_some();

    if !already_applied {
        transaction
            .execute_unprepared(EMAIL_SQL)
            .await
            .map_err(OrmError::from_db_err)?;
        transaction
            .execute_unprepared(EVENT_SQL)
            .await
            .map_err(OrmError::from_db_err)?;
        transaction
            .execute(Statement::from_sql_and_values(
                transaction.get_database_backend(),
                "INSERT INTO zed_schema_migrations(version) VALUES ($1)",
                [VERSION.into()],
            ))
            .await
            .map_err(OrmError::from_db_err)?;
    }

    transaction.commit().await.map_err(OrmError::from_db_err)?;
    Ok(NotificationMigrationReport {
        version: VERSION,
        applied: !already_applied,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_is_forward_only_and_product_owned() {
        assert_eq!(VERSION, "package-email-notifications@2026-09-16-v1");
        assert!(EMAIL_SQL.contains("zed_package_favorites"));
        assert!(EMAIL_SQL.contains("zed_notification_preferences"));
        assert!(EMAIL_SQL.contains("zed_package_security_advisories"));
        assert!(EMAIL_SQL.contains("zed_email_notification_outbox"));
        assert!(EVENT_SQL.contains("zed_package_notification_events"));
        assert!(!EMAIL_SQL.to_ascii_lowercase().contains("sendgrid_api_key"));
        assert!(!EVENT_SQL.to_ascii_lowercase().contains("sendgrid_api_key"));
        assert!(!EMAIL_SQL.to_ascii_lowercase().contains("nats_url"));
    }
}
