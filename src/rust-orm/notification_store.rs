//! Durable product-side state mutations for package notifications.
//!
//! This module contains no SendGrid, fanwaave, NATS, SMTP, or Supabase
//! transport logic. It owns user intent, immutable package-update facts, and
//! safe leasing/reconciliation of already-rendered outbox rows.

use uuid::Uuid;

#[cfg(feature = "read-write")]
use sea_orm::{ConnectionTrait, Statement};

#[cfg(feature = "read-write")]
use crate::{connection::WriteContext, error::OrmError, notifications::OutboxEmail};

const MAX_OUTBOX_BATCH: u64 = 500;
const MAX_LEASE_SECONDS: u64 = 15 * 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationPreferences {
    pub email_enabled: bool,
    pub favorite_package_email: bool,
    pub downloaded_package_email: bool,
    pub major_release_email: bool,
    pub security_email: bool,
    pub digest_email: bool,
    pub digest_frequency: String,
    pub minimum_security_severity: String,
    pub timezone: String,
    pub digest_hour: i16,
}

impl Default for NotificationPreferences {
    fn default() -> Self {
        Self {
            email_enabled: true,
            favorite_package_email: true,
            downloaded_package_email: true,
            major_release_email: true,
            security_email: true,
            digest_email: true,
            digest_frequency: "weekly".to_owned(),
            minimum_security_severity: "high".to_owned(),
            timezone: "UTC".to_owned(),
            digest_hour: 9,
        }
    }
}

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

#[cfg(feature = "read-write")]
pub async fn set_package_favorite(
    context: &WriteContext,
    user_id: Uuid,
    package_id: Uuid,
    favorite: bool,
) -> Result<(), OrmError> {
    let statement = if favorite {
        Statement::from_sql_and_values(
            context.connection().get_database_backend(),
            "insert into zed_package_favorites (user_id, package_id) values ($1, $2) on conflict (user_id, package_id) do nothing",
            [user_id.into(), package_id.into()],
        )
    } else {
        Statement::from_sql_and_values(
            context.connection().get_database_backend(),
            "delete from zed_package_favorites where user_id = $1 and package_id = $2",
            [user_id.into(), package_id.into()],
        )
    };
    context
        .connection()
        .execute(statement)
        .await
        .map_err(OrmError::from_db_err)?;
    Ok(())
}

#[cfg(feature = "read-write")]
pub async fn load_notification_preferences(
    context: &WriteContext,
    user_id: Uuid,
) -> Result<NotificationPreferences, OrmError> {
    let row = context
        .connection()
        .query_one(Statement::from_sql_and_values(
            context.connection().get_database_backend(),
            "select email_enabled, favorite_package_email, downloaded_package_email, major_release_email, security_email, digest_email, digest_frequency, minimum_security_severity, timezone, digest_hour from zed_notification_preferences where user_id = $1",
            [user_id.into()],
        ))
        .await
        .map_err(OrmError::from_db_err)?;
    let Some(row) = row else {
        return Ok(NotificationPreferences::default());
    };
    Ok(NotificationPreferences {
        email_enabled: row.try_get("", "email_enabled").map_err(OrmError::from_db_err)?,
        favorite_package_email: row
            .try_get("", "favorite_package_email")
            .map_err(OrmError::from_db_err)?,
        downloaded_package_email: row
            .try_get("", "downloaded_package_email")
            .map_err(OrmError::from_db_err)?,
        major_release_email: row
            .try_get("", "major_release_email")
            .map_err(OrmError::from_db_err)?,
        security_email: row.try_get("", "security_email").map_err(OrmError::from_db_err)?,
        digest_email: row.try_get("", "digest_email").map_err(OrmError::from_db_err)?,
        digest_frequency: row
            .try_get("", "digest_frequency")
            .map_err(OrmError::from_db_err)?,
        minimum_security_severity: row
            .try_get("", "minimum_security_severity")
            .map_err(OrmError::from_db_err)?,
        timezone: row.try_get("", "timezone").map_err(OrmError::from_db_err)?,
        digest_hour: row.try_get("", "digest_hour").map_err(OrmError::from_db_err)?,
    })
}

#[cfg(feature = "read-write")]
pub async fn upsert_notification_preferences(
    context: &WriteContext,
    user_id: Uuid,
    preferences: &NotificationPreferences,
) -> Result<(), OrmError> {
    validate_preferences(preferences)?;
    context
        .connection()
        .execute(Statement::from_sql_and_values(
            context.connection().get_database_backend(),
            r#"
            insert into zed_notification_preferences (
              user_id, email_enabled, favorite_package_email,
              downloaded_package_email, major_release_email, security_email,
              digest_email, digest_frequency, minimum_security_severity,
              timezone, digest_hour, unsubscribed_at
            ) values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,
                      case when $2 then null else now() end)
            on conflict (user_id) do update set
              email_enabled = excluded.email_enabled,
              favorite_package_email = excluded.favorite_package_email,
              downloaded_package_email = excluded.downloaded_package_email,
              major_release_email = excluded.major_release_email,
              security_email = excluded.security_email,
              digest_email = excluded.digest_email,
              digest_frequency = excluded.digest_frequency,
              minimum_security_severity = excluded.minimum_security_severity,
              timezone = excluded.timezone,
              digest_hour = excluded.digest_hour,
              unsubscribed_at = case when excluded.email_enabled then null else coalesce(zed_notification_preferences.unsubscribed_at, now()) end
            "#,
            [
                user_id.into(),
                preferences.email_enabled.into(),
                preferences.favorite_package_email.into(),
                preferences.downloaded_package_email.into(),
                preferences.major_release_email.into(),
                preferences.security_email.into(),
                preferences.digest_email.into(),
                preferences.digest_frequency.clone().into(),
                preferences.minimum_security_severity.clone().into(),
                preferences.timezone.clone().into(),
                preferences.digest_hour.into(),
            ],
        ))
        .await
        .map_err(OrmError::from_db_err)?;

    if !preferences.email_enabled {
        suppress_pending_outbox_for_user(context, user_id).await?;
    }
    Ok(())
}

fn validate_preferences(preferences: &NotificationPreferences) -> Result<(), OrmError> {
    if !matches!(preferences.digest_frequency.as_str(), "daily" | "weekly" | "never") {
        return Err(OrmError::policy("digest_frequency must be daily, weekly, or never"));
    }
    if !matches!(
        preferences.minimum_security_severity.as_str(),
        "moderate" | "high" | "critical"
    ) {
        return Err(OrmError::policy(
            "minimum_security_severity must be moderate, high, or critical",
        ));
    }
    if preferences.timezone.trim().is_empty() || preferences.timezone.len() > 96 {
        return Err(OrmError::policy("timezone must be 1 to 96 bytes"));
    }
    if !(0..=23).contains(&preferences.digest_hour) {
        return Err(OrmError::policy("digest_hour must be between 0 and 23"));
    }
    Ok(())
}

#[cfg(feature = "read-write")]
pub async fn record_security_advisory(
    context: &WriteContext,
    input: &SecurityAdvisoryInput,
) -> Result<Uuid, OrmError> {
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

#[cfg(feature = "read-write")]
pub async fn record_notification_event(
    context: &WriteContext,
    input: &NotificationEventInput,
) -> Result<bool, OrmError> {
    if !matches!(input.event_type, "major_release" | "security_patch") {
        return Err(OrmError::policy("unsupported package notification event type"));
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

#[cfg(feature = "read-write")]
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

    rows.into_iter()
        .map(|row| {
            Ok(OutboxEmail {
                id: row.try_get("", "id").map_err(OrmError::from_db_err)?,
                user_id: row.try_get("", "user_id").map_err(OrmError::from_db_err)?,
                package_id: row.try_get("", "package_id").map_err(OrmError::from_db_err)?,
                event_type: row.try_get("", "event_type").map_err(OrmError::from_db_err)?,
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
                text_body: row.try_get("", "text_body").map_err(OrmError::from_db_err)?,
                html_body: row.try_get("", "html_body").map_err(OrmError::from_db_err)?,
                attempt_count: row
                    .try_get("", "attempt_count")
                    .map_err(OrmError::from_db_err)?,
                metadata: row.try_get("", "metadata").map_err(OrmError::from_db_err)?,
            })
        })
        .collect()
}

#[cfg(feature = "read-write")]
pub async fn recover_expired_outbox_leases(
    context: &WriteContext,
) -> Result<u64, OrmError> {
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

#[cfg(feature = "read-write")]
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

#[cfg(feature = "read-write")]
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

#[cfg(feature = "read-write")]
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

#[cfg(feature = "read-write")]
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

#[cfg(feature = "read-write")]
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
    fn default_preferences_enable_useful_mail_without_lowering_security_threshold() {
        let preferences = NotificationPreferences::default();
        assert!(preferences.email_enabled);
        assert!(preferences.major_release_email);
        assert!(preferences.security_email);
        assert_eq!(preferences.minimum_security_severity, "high");
        assert_eq!(preferences.digest_frequency, "weekly");
    }

    #[test]
    fn rejects_invalid_preference_values_before_sql() {
        let mut preferences = NotificationPreferences::default();
        preferences.digest_frequency = "hourly".to_owned();
        assert!(validate_preferences(&preferences).is_err());
        preferences.digest_frequency = "daily".to_owned();
        preferences.digest_hour = 24;
        assert!(validate_preferences(&preferences).is_err());
    }
}
