//! Read-only, policy-aware named query functions.
//!
//! This module is the default consumer's entire view of the database. Business
//! reads added here must carry tenant/user scope and apply redaction. Generated
//! entities and raw query builders stay private to this crate. Prefer
//! `get_published_items_for_tenant(tenant_id)`-style named contracts over
//! anything that hands a caller a query builder.

use crate::{
    connection::{inspect_connection, InternalConnectionState},
    OrmError, ReadContext,
};

/// Safe, implementation-independent evidence about the active connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionState {
    schema: String,
    transaction_read_only: bool,
}

impl ConnectionState {
    pub(crate) fn from_internal(state: InternalConnectionState) -> Self {
        Self {
            schema: state.schema,
            transaction_read_only: state.transaction_read_only,
        }
    }

    #[must_use]
    pub fn schema(&self) -> &str {
        &self.schema
    }

    #[must_use]
    pub fn transaction_read_only(&self) -> bool {
        self.transaction_read_only
    }
}

/// Return the verified policy state without exposing a SeaORM connection.
pub async fn connection_state(context: &ReadContext) -> Result<ConnectionState, OrmError> {
    inspect_connection(context.connection())
        .await
        .map(ConnectionState::from_internal)
}

/// Lightweight named readiness read for consumers and health checks.
pub async fn ping(context: &ReadContext) -> Result<(), OrmError> {
    let state = connection_state(context).await?;
    if state.transaction_read_only() {
        Ok(())
    } else {
        Err(OrmError::policy(
            "read context lost its read-only transaction policy",
        ))
    }
}
