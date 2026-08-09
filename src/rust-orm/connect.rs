//! Role-aware connection construction for the registry schema.

use std::time::Duration;

use sea_orm::{ConnectOptions, Database, DatabaseConnection, DbErr};

use crate::schema::REGISTRY_SCHEMA;

/// Database authority requested by a service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbRole {
    /// API server, migration job, and explicitly authorized background workers.
    ReadWrite,
    /// Browser-facing read paths and reporting workers.
    ReadOnly,
}

const READ_ONLY_OPTIONS: &str = "options=-c%20default_transaction_read_only%3Don";

/// Apply the Postgres read-only startup option when requested.
pub fn apply_role(database_url: &str, role: DbRole) -> String {
    match role {
        DbRole::ReadWrite => database_url.to_owned(),
        DbRole::ReadOnly => {
            let separator = if database_url.contains('?') { '&' } else { '?' };
            format!("{database_url}{separator}{READ_ONLY_OPTIONS}")
        }
    }
}

/// Connect with bounded pool defaults and a deterministic schema search path.
pub async fn connect(database_url: &str, role: DbRole) -> Result<DatabaseConnection, DbErr> {
    let mut options = ConnectOptions::new(apply_role(database_url, role));
    options
        .max_connections(10)
        .min_connections(1)
        .connect_timeout(Duration::from_secs(10))
        .acquire_timeout(Duration::from_secs(10))
        .idle_timeout(Duration::from_secs(300))
        .sqlx_logging(false)
        .set_schema_search_path(REGISTRY_SCHEMA);
    Database::connect(options).await
}

/// Verify that a read-only pool did not silently lose its startup setting.
pub async fn assert_read_only(conn: &DatabaseConnection) -> Result<(), DbErr> {
    use sea_orm::{ConnectionTrait, Statement};

    let statement = Statement::from_string(
        conn.get_database_backend(),
        "SELECT current_setting('default_transaction_read_only') AS setting",
    );
    let row = conn
        .query_one(statement)
        .await?
        .ok_or_else(|| DbErr::Custom("default_transaction_read_only returned no row".into()))?;
    let setting: String = row.try_get("", "setting")?;
    if setting == "on" {
        Ok(())
    } else {
        Err(DbErr::Custom(format!(
            "connection is not read-only: default_transaction_read_only = {setting:?}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::{DbRole, apply_role};

    #[test]
    fn read_only_appends_options_to_plain_url() {
        assert_eq!(
            apply_role("postgres://app@db/zed", DbRole::ReadOnly),
            "postgres://app@db/zed?options=-c%20default_transaction_read_only%3Don"
        );
    }

    #[test]
    fn read_only_respects_existing_query_string() {
        assert_eq!(
            apply_role("postgres://app@db/zed?sslmode=require", DbRole::ReadOnly),
            "postgres://app@db/zed?sslmode=require&options=-c%20default_transaction_read_only%3Don"
        );
    }

    #[test]
    fn read_write_passes_url_through() {
        let url = "postgres://app@db/zed?sslmode=require";
        assert_eq!(apply_role(url, DbRole::ReadWrite), url);
    }
}
