//! Write surface — API servers only (`read-write` feature).
//!
//! Every symbol in this module disappears from default builds. Add only named
//! mutations that preserve API-owned authorization, validation, invariants,
//! audit behavior, and transaction boundaries; never expose a raw ORM session.
//! The feature gate expresses intent; the authoritative control is the
//! database principal, whose grants deny web identities all DML.

use crate::{connection::inspect_connection, read::ConnectionState, OrmError, WriteContext};

/// Return safe policy evidence for the API's opaque write context.
pub async fn connection_state(context: &WriteContext) -> Result<ConnectionState, OrmError> {
    inspect_connection(context.connection())
        .await
        .map(ConnectionState::from_internal)
}

/// Lightweight readiness check for an API consumer.
pub async fn ping(context: &WriteContext) -> Result<(), OrmError> {
    let state = connection_state(context).await?;
    if state.transaction_read_only() {
        Err(OrmError::policy(
            "write context unexpectedly became transaction read-only",
        ))
    } else {
        Ok(())
    }
}
