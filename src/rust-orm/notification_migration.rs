//! Forward-only migrations for Zed package email notification state.
//!
//! Kept separate from the historical registry migration series so the feature
//! can ship without replaying or mutating the established registry ledger.
//! The API server's discrete `migrate` command invokes this after the canonical
//! registry migration batch.

use sea_orm::{ConnectionTrait, Statement, TransactionTrait};

use crate::{connection::WriteContext, error::OrmError};

const EMAIL_SQL: &str = include_str!("sql/2026-09-16-package-email-notifications.sql");
const EVENT_SQL: &str = include_str!("sql/2026-09-16-package-notification-events.sql");
const LEASE_SQL: &str = include_str!("sql/2026-09-16-package-email-outbox-leases.sql");
const BASE_VERSION: &str = "package-email-notifications@2026-09-16-v1";
const LEASE_VERSION: &str = "package-email-notifications@2026-09-16-v2-outbox-leases";
const MIGRATION_LOCK_KEY: i64 = 913_447_316;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotificationMigrationReport {
    pub version: &'static str,
    pub applied: bool,
}

pub const fn notification_migration_version() -> &'static str {
    LEASE_VERSION
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

    let base_applied = ledger_contains(&transaction, BASE_VERSION).await?;
    let lease_applied = ledger_contains(&transaction, LEASE_VERSION).await?;
    let mut applied = false;

    if !base_applied {
        transaction
            .execute_unprepared(EMAIL_SQL)
            .await
            .map_err(OrmError::from_db_err)?;
        transaction
            .execute_unprepared(EVENT_SQL)
            .await
            .map_err(OrmError::from_db_err)?;
        record_version(&transaction, BASE_VERSION).await?;
        applied = true;
    }

    if !lease_applied {
        transaction
            .execute_unprepared(LEASE_SQL)
            .await
            .map_err(OrmError::from_db_err)?;
        record_version(&transaction, LEASE_VERSION).await?;
        applied = true;
    }

    transaction.commit().await.map_err(OrmError::from_db_err)?;
    Ok(NotificationMigrationReport {
        version: LEASE_VERSION,
        applied,
    })
}

async fn ledger_contains<C: ConnectionTrait>(
    connection: &C,
    version: &str,
) -> Result<bool, OrmError> {
    Ok(connection
        .query_one(Statement::from_sql_and_values(
            connection.get_database_backend(),
            "SELECT version FROM zed_schema_migrations WHERE version = $1",
            [version.to_owned().into()],
        ))
        .await
        .map_err(OrmError::from_db_err)?
        .is_some())
}

async fn record_version<C: ConnectionTrait>(connection: &C, version: &str) -> Result<(), OrmError> {
    connection
        .execute(Statement::from_sql_and_values(
            connection.get_database_backend(),
            "INSERT INTO zed_schema_migrations(version) VALUES ($1)",
            [version.to_owned().into()],
        ))
        .await
        .map_err(OrmError::from_db_err)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_series_is_forward_only_and_product_owned() {
        assert_eq!(BASE_VERSION, "package-email-notifications@2026-09-16-v1");
        assert_eq!(
            notification_migration_version(),
            "package-email-notifications@2026-09-16-v2-outbox-leases"
        );
        assert!(EMAIL_SQL.contains("zed_package_favorites"));
        assert!(EMAIL_SQL.contains("zed_notification_preferences"));
        assert!(EMAIL_SQL.contains("zed_package_security_advisories"));
        assert!(EMAIL_SQL.contains("zed_email_notification_outbox"));
        assert!(EVENT_SQL.contains("zed_package_notification_events"));
        assert!(LEASE_SQL.contains("lease_expires_at"));
        assert!(!EMAIL_SQL.to_ascii_lowercase().contains("sendgrid_api_key"));
        assert!(!EVENT_SQL.to_ascii_lowercase().contains("sendgrid_api_key"));
        assert!(!LEASE_SQL.to_ascii_lowercase().contains("sendgrid_api_key"));
        assert!(!EMAIL_SQL.to_ascii_lowercase().contains("nats_url"));
    }
}
