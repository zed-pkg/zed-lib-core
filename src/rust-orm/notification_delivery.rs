//! Durable event/advisory state and crash-safe outbox leasing for Zed email.
//!
//! Product semantics remain in Zed. This module intentionally contains no
//! SendGrid, SMTP, fanwaave, NATS, or provider credentials.

use uuid::Uuid;

use sea_orm::{ConnectionTrait, Statement};

use crate::{connection::WriteContext, error::OrmError, notifications::OutboxEmail};

const MAX_OUTBOX_BATCH: u64 = 500;
const MAX_LEASE_SECONDS: u64 = 15 * 60;

#[derive(Debug, Clone, PartialEq)]
pub struct SecurityAdvisoryInput {
    pub package_id: Uuid,
    pub advisory_id: String,
    pub advisory_url: String,
    pub affected_version: String,
    pub fixed_version: String,
    pub severity: String,
    pub summary: Option<String>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NotificationEventInput {
    pub package_id: Uuid,
    pub event_type: &'static str,
    pub event_key: String,
    pub previous_version: Option<String>,
    pub current_version: String,
    pub security_advisory_id: Option<Uuid>,
    pub metadata: serde_json::Value,
}

pub async fn record_security_advisory(
    context: &WriteContext,
    input: &SecurityAdvisoryInput,
) -> Result<Uuid, OrmError> {
    validate_security_advisory(input)?;
    let row = context
        .connection()
        .query_one(Statement::from_sql_and_values(
            context.connection().get_database_backend(),
            r#"
            with inserted as (
              insert into zed_package_security_advisories (
                id, package_id, advisory_id, advisory_url, affected_version,
                fixed_version, severity, summary, metadata
              ) values ($1,$2,$3,$4,$5,$6,$7,$8,$9)
              on conflict (package_id, advisory_id, fixed_version) do nothing
              returning id
            )
            select id from inserted
            union all
            select id from zed_package_security_advisories
             where package_id = $2 and advisory_id = $3 and fixed_version = $6
            limit 1
            "#,
            [
                Uuid::new_v4().into(),
                input.package_id.into(),
                input.advisory_id.clone().into(),
                input.advisory_url.clone().into(),
                input.affected_version.clone().into(),
                input.fixed_version.clone().into(),
                input.severity.clone().into(),
                input.summary.clone().into(),
                input.metadata.clone().into(),
            ],
        ))
        .await
        .map_err(OrmError::from_db_err)?
        .ok_or_else(|| OrmError::policy("security advisory insert returned no identity"))?;
    row.try_get("", "id").map_err(OrmError::from_db_err)
}

fn validate_security_advisory(input: &SecurityAdvisoryInput) -> Result<(), OrmError> {
    if !matches!(input.severity.as_str(), "moderate" | "high" | "critical") {
        return Err(OrmError::policy(
            "security advisory severity must be moderate, high, or critical",
        ));
    }
    if input.advisory_id.trim().is_empty() || input.advisory_id.len() > 160 {
        return Err(OrmError::policy("advisory_id must contain 1 to 160 bytes"));
    }
    if !input.advisory_url.starts_with("https://") || input.advisory_url.len() > 2048 {
        return Err(OrmError::policy(
            "advisory_url must be an HTTPS URL of at most 2048 bytes",
        ));
    }
    Ok(())
}

pub async fn record_notification_event(
    context: &WriteContext,
    input: &NotificationEventInput,
) -> Result<bool, OrmError> {
    if !matches!(input.event_type, "major_release" | "security_patch") {
        return Err(OrmError::policy(
            "unsupported package notification event type",
        ));
    }
    if input.event_key.is_empty() || input.event_key.len() > 255 {
        return Err(OrmError::policy(
            "notification event key must contain 1 to 255 bytes",
        ));
    }
    if (input.event_type == "security_patch") != input.security_advisory_id.is_some() {
        return Err(OrmError::policy(
            "security_patch events must reference exactly one advisory",
        ));
    }
    let result = context
        .connection()
        .execute(Statement::from_sql_and_values(
            context.connection().get_database_backend(),
            r#"
            insert into zed_package_notification_events (
              id, package_id, event_type, event_key, previous_version,
              current_version, security_advisory_id, occurred_at, metadata
            ) values ($1,$2,$3,$4,$5,$6,$7,now(),$8)
            on conflict (event_key) do nothing
            "#,
            [
                Uuid::new_v4().into(),
                input.package_id.into(),
                input.event_type.into(),
                input.event_key.clone().into(),
                input.previous_version.clone().into(),
                input.current_version.clone().into(),
                input.security_advisory_id.into(),
                input.metadata.clone().into(),
            ],
        ))
        .await
        .map_err(OrmError::from_db_err)?;
    Ok(result.rows_affected() == 1)
}

pub async fn claim_outbox_batch_with_lease(
    context: &WriteContext,
    limit: u64,
    lease_seconds: u64,
) -> Result<Vec<OutboxEmail>, OrmError> {
    let limit = limit.clamp(1, MAX_OUTBOX_BATCH);
    let lease_seconds = lease_seconds.clamp(10, MAX_LEASE_SECONDS) as i64;
    let rows = context
        .connection()
        .query_all(Statement::from_sql_and_values(
            context.connection().get_database_backend(),
            r#"
            with claimed as (
              select id
                from zed_email_notification_outbox
               where state = 'pending'
                 and available_at <= now()
               order by available_at, queued_at, id
               for update skip locked
               limit $1
            )
            update zed_email_notification_outbox outbox
               set state = 'publishing',
                   attempt_count = attempt_count + 1,
                   lease_expires_at = now() + ($2::bigint * interval '1 second')
              from claimed
             where outbox.id = claimed.id
            returning outbox.id, outbox.user_id, outbox.package_id,
                      outbox.event_type, outbox.idempotency_key,
                      outbox.recipient_email, outbox.recipient_name,
                      outbox.subject, outbox.text_body, outbox.html_body,
                      outbox.attempt_count, outbox.metadata
            "#,
            [(limit as i64).into(), lease_seconds.into()],
        ))
        .await
        .map_err(OrmError::from_db_err)?;

    rows.into_iter().map(outbox_from_row).collect()
}

fn outbox_from_row(row: sea_orm::QueryResult) -> Result<OutboxEmail, OrmError> {
    Ok(OutboxEmail {
        id: row.try_get("", "id").map_err(OrmError::from_db_err)?,
        user_id: row.try_get("", "user_id").map_err(OrmError::from_db_err)?,
        package_id: row
            .try_get("", "package_id")
            .map_err(OrmError::from_db_err)?,
        event_type: row
            .try_get("", "event_type")
            .map_err(OrmError::from_db_err)?,
        idempotency_key: row
            .try_get("", "idempotency_key")
            .map_err(OrmError::from_db_err)?,
        recipient_email: row
            .try_get("", "recipient_email")
            .map_err(OrmError::from_db_err)?,
        recipient_name: row
            .try_get("", "recipient_name")
            .map_err(OrmError::from_db_err)?,
        subject: row.try_get("", "subject").map_err(OrmError::from_db_err)?,
        text_body: row
            .try_get("", "text_body")
            .map_err(OrmError::from_db_err)?,
        html_body: row
            .try_get("", "html_body")
            .map_err(OrmError::from_db_err)?,
        attempt_count: row
            .try_get("", "attempt_count")
            .map_err(OrmError::from_db_err)?,
        metadata: row.try_get("", "metadata").map_err(OrmError::from_db_err)?,
    })
}

pub async fn recover_expired_outbox_leases(context: &WriteContext) -> Result<u64, OrmError> {
    let result = context
        .connection()
        .execute(Statement::from_string(
            context.connection().get_database_backend(),
            "update zed_email_notification_outbox set state = 'pending', lease_expires_at = null, available_at = now(), last_error_code = 'lease_expired' where state = 'publishing' and lease_expires_at <= now()",
        ))
        .await
        .map_err(OrmError::from_db_err)?;
    Ok(result.rows_affected())
}

pub async fn mark_outbox_published(
    context: &WriteContext,
    id: Uuid,
    provider_message_id: Option<&str>,
) -> Result<bool, OrmError> {
    let result = context
        .connection()
        .execute(Statement::from_sql_and_values(
            context.connection().get_database_backend(),
            "update zed_email_notification_outbox set state = 'published', published_at = now(), lease_expires_at = null, provider_message_id = left($2, 512), last_error_code = null where id = $1 and state = 'publishing'",
            [id.into(), provider_message_id.into()],
        ))
        .await
        .map_err(OrmError::from_db_err)?;
    Ok(result.rows_affected() == 1)
}

pub async fn mark_outbox_delivered(
    context: &WriteContext,
    id: Uuid,
    provider_message_id: Option<&str>,
) -> Result<bool, OrmError> {
    let result = context
        .connection()
        .execute(Statement::from_sql_and_values(
            context.connection().get_database_backend(),
            "update zed_email_notification_outbox set state = 'delivered', completed_at = now(), lease_expires_at = null, provider_message_id = left($2, 512), last_error_code = null where id = $1 and state in ('publishing','published')",
            [id.into(), provider_message_id.into()],
        ))
        .await
        .map_err(OrmError::from_db_err)?;
    Ok(result.rows_affected() == 1)
}

pub async fn release_outbox_for_retry(
    context: &WriteContext,
    id: Uuid,
    error_code: &str,
    retry_after_seconds: u64,
) -> Result<bool, OrmError> {
    let retry_after_seconds = retry_after_seconds.clamp(1, 86_400) as i64;
    let result = context
        .connection()
        .execute(Statement::from_sql_and_values(
            context.connection().get_database_backend(),
            "update zed_email_notification_outbox set state = 'pending', lease_expires_at = null, available_at = now() + ($2::bigint * interval '1 second'), last_error_code = left($3, 128) where id = $1 and state = 'publishing'",
            [id.into(), retry_after_seconds.into(), error_code.into()],
        ))
        .await
        .map_err(OrmError::from_db_err)?;
    Ok(result.rows_affected() == 1)
}

pub async fn mark_outbox_failed(
    context: &WriteContext,
    id: Uuid,
    error_code: &str,
) -> Result<bool, OrmError> {
    let result = context
        .connection()
        .execute(Statement::from_sql_and_values(
            context.connection().get_database_backend(),
            "update zed_email_notification_outbox set state = 'failed', completed_at = now(), lease_expires_at = null, last_error_code = left($2, 128) where id = $1 and state in ('publishing','published')",
            [id.into(), error_code.into()],
        ))
        .await
        .map_err(OrmError::from_db_err)?;
    Ok(result.rows_affected() == 1)
}

pub async fn suppress_pending_outbox_for_user(
    context: &WriteContext,
    user_id: Uuid,
) -> Result<u64, OrmError> {
    let result = context
        .connection()
        .execute(Statement::from_sql_and_values(
            context.connection().get_database_backend(),
            "update zed_email_notification_outbox set state = 'suppressed', completed_at = now(), lease_expires_at = null, last_error_code = 'user_opted_out' where user_id = $1 and state in ('pending','publishing')",
            [user_id.into()],
        ))
        .await
        .map_err(OrmError::from_db_err)?;
    Ok(result.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advisory_validation_is_bounded_and_https_only() {
        let input = SecurityAdvisoryInput {
            package_id: Uuid::nil(),
            advisory_id: "GHSA-example".into(),
            advisory_url: "https://example.test/advisory".into(),
            affected_version: "1.0.0".into(),
            fixed_version: "1.0.1".into(),
            severity: "high".into(),
            summary: None,
            metadata: serde_json::json!({}),
        };
        assert!(validate_security_advisory(&input).is_ok());
        let mut bad = input.clone();
        bad.advisory_url = "http://example.test".into();
        assert!(validate_security_advisory(&bad).is_err());
    }
}
