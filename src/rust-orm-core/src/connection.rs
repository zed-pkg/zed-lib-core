use std::{fmt, time::Duration};

use sea_orm::sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sea_orm::{ConnectionTrait, DatabaseConnection, SqlxPostgresConnector, Statement};

use crate::{error::OrmError, schema::ORG_SCHEMA};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectPolicy {
    max_connections: u32,
    min_connections: u32,
    acquire_timeout: Duration,
    idle_timeout: Duration,
}

impl Default for ConnectPolicy {
    fn default() -> Self {
        Self {
            max_connections: 10,
            min_connections: 1,
            acquire_timeout: Duration::from_secs(10),
            idle_timeout: Duration::from_secs(300),
        }
    }
}

impl ConnectPolicy {
    #[must_use]
    pub fn with_max_connections(mut self, value: u32) -> Self {
        self.max_connections = value.max(1);
        self.min_connections = self.min_connections.min(self.max_connections);
        self
    }

    #[must_use]
    pub fn with_min_connections(mut self, value: u32) -> Self {
        self.min_connections = value.min(self.max_connections);
        self
    }

    #[must_use]
    pub fn with_acquire_timeout(mut self, value: Duration) -> Self {
        self.acquire_timeout = value;
        self
    }

    #[must_use]
    pub fn with_idle_timeout(mut self, value: Duration) -> Self {
        self.idle_timeout = value;
        self
    }
}

#[derive(Clone)]
pub struct ReadContext {
    connection: DatabaseConnection,
}

impl fmt::Debug for ReadContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReadContext")
            .field("schema", &ORG_SCHEMA)
            .finish_non_exhaustive()
    }
}

impl ReadContext {
    pub(crate) fn connection(&self) -> &DatabaseConnection {
        &self.connection
    }
}

#[cfg(feature = "read-write")]
#[derive(Clone)]
pub struct WriteContext {
    connection: DatabaseConnection,
}

#[cfg(feature = "read-write")]
impl fmt::Debug for WriteContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WriteContext")
            .field("schema", &ORG_SCHEMA)
            .finish_non_exhaustive()
    }
}

#[cfg(feature = "read-write")]
impl WriteContext {
    pub(crate) fn connection(&self) -> &DatabaseConnection {
        &self.connection
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InternalConnectionState {
    pub(crate) schema: String,
    pub(crate) transaction_read_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    ReadOnly,
    #[cfg(feature = "read-write")]
    ReadWrite,
}

/// Open an opaque context whose every transaction starts read-only.
pub async fn connect_read_only(database_url: &str) -> Result<ReadContext, OrmError> {
    connect_read_only_with_policy(database_url, ConnectPolicy::default()).await
}

/// Open an opaque read context with an explicit pool policy.
pub async fn connect_read_only_with_policy(
    database_url: &str,
    policy: ConnectPolicy,
) -> Result<ReadContext, OrmError> {
    let connection = connect(database_url, policy, Role::ReadOnly).await?;
    Ok(ReadContext { connection })
}

/// Open an opaque write context. This symbol does not exist unless the caller
/// explicitly enables the `read-write` feature.
#[cfg(feature = "read-write")]
pub async fn connect_read_write(database_url: &str) -> Result<WriteContext, OrmError> {
    connect_read_write_with_policy(database_url, ConnectPolicy::default()).await
}

/// Open an opaque write context with an explicit pool policy.
#[cfg(feature = "read-write")]
pub async fn connect_read_write_with_policy(
    database_url: &str,
    policy: ConnectPolicy,
) -> Result<WriteContext, OrmError> {
    let connection = connect(database_url, policy, Role::ReadWrite).await?;
    Ok(WriteContext { connection })
}

async fn connect(
    database_url: &str,
    policy: ConnectPolicy,
    role: Role,
) -> Result<DatabaseConnection, OrmError> {
    let options = database_url
        .parse::<PgConnectOptions>()
        .map_err(OrmError::database)?
        .options(startup_options(role));

    let pool = PgPoolOptions::new()
        .max_connections(policy.max_connections)
        .min_connections(policy.min_connections)
        .acquire_timeout(policy.acquire_timeout)
        .idle_timeout(Some(policy.idle_timeout))
        .connect_with(options)
        .await
        .map_err(OrmError::database)?;

    let connection = SqlxPostgresConnector::from_sqlx_postgres_pool(pool);
    let state = inspect_connection(&connection).await?;
    if state.schema != ORG_SCHEMA {
        return Err(OrmError::policy(format!(
            "search_path resolved to schema {:?}; expected {ORG_SCHEMA:?}",
            state.schema
        )));
    }

    match role {
        Role::ReadOnly if !state.transaction_read_only => {
            return Err(OrmError::policy(
                "read context did not verify default_transaction_read_only=on",
            ));
        }
        #[cfg(feature = "read-write")]
        Role::ReadWrite if state.transaction_read_only => {
            return Err(OrmError::policy(
                "write context resolved to a read-only transaction policy",
            ));
        }
        _ => {}
    }

    Ok(connection)
}

fn startup_options(role: Role) -> Vec<(&'static str, String)> {
    let mut options = vec![("search_path", ORG_SCHEMA.to_owned())];
    if role == Role::ReadOnly {
        options.push(("default_transaction_read_only", "on".to_owned()));
    }
    options
}

pub(crate) async fn inspect_connection(
    connection: &DatabaseConnection,
) -> Result<InternalConnectionState, OrmError> {
    let statement = Statement::from_string(
        connection.get_database_backend(),
        "SELECT current_schema() AS schema_name, \
         current_setting('default_transaction_read_only') AS transaction_read_only",
    );
    let row = connection
        .query_one(statement)
        .await
        .map_err(OrmError::database)?
        .ok_or_else(|| OrmError::policy("connection policy query returned no row"))?;

    let schema = row
        .try_get::<Option<String>>("", "schema_name")
        .map_err(OrmError::database)?
        .ok_or_else(|| {
            OrmError::policy(format!(
                "no usable current_schema(); expected schema {ORG_SCHEMA:?} with USAGE"
            ))
        })?;
    let read_only = row
        .try_get::<String>("", "transaction_read_only")
        .map_err(OrmError::database)?;

    Ok(InternalConnectionState {
        schema,
        transaction_read_only: read_only == "on",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_startup_options_pin_schema_and_transaction_policy() {
        let options = startup_options(Role::ReadOnly);
        assert!(options.contains(&("search_path", ORG_SCHEMA.to_owned())));
        assert!(options.contains(&("default_transaction_read_only", "on".to_owned())));
    }

    #[cfg(feature = "read-write")]
    #[test]
    fn read_write_startup_options_do_not_claim_read_only() {
        let options = startup_options(Role::ReadWrite);
        assert!(options.contains(&("search_path", ORG_SCHEMA.to_owned())));
        assert!(!options
            .iter()
            .any(|(key, _)| *key == "default_transaction_read_only"));
    }

    #[tokio::test]
    #[ignore = "requires a dedicated ORM_CORE_TEST_DATABASE_URL database"]
    async fn live_read_only_context_rejects_schema_ddl() {
        let database_url = std::env::var("ORM_CORE_TEST_DATABASE_URL")
            .expect("ORM_CORE_TEST_DATABASE_URL must target a disposable test database");
        let context = connect_read_only(&database_url)
            .await
            .expect("read-only connection must verify");
        let backend = context.connection().get_database_backend();
        let table = format!("{ORG_SCHEMA}.__orm_core_forbidden_write_probe");
        let result = context
            .connection()
            .execute(Statement::from_string(
                backend,
                format!("CREATE TABLE {table} (id integer primary key)"),
            ))
            .await;

        if result.is_ok() {
            let _ = context
                .connection()
                .execute(Statement::from_string(
                    backend,
                    format!("DROP TABLE IF EXISTS {table}"),
                ))
                .await;
            panic!("read-only context unexpectedly executed DDL");
        }
    }
}
