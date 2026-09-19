//! Zed-owned package notification policy, rendering, and durable outbox.
//!
//! This module intentionally knows nothing about SendGrid credentials, NATS,
//! fanwaave deployment topology, or provider retry rules. Zed decides who
//! should receive a message and renders the exact immutable subject/text/HTML
//! stored in `zed_email_notification_outbox`. A separate dispatcher can then
//! hand that rendered message to a generic delivery service.

use uuid::Uuid;

#[cfg(feature = "read-write")]
use sea_orm::{ConnectionTrait, Statement};

#[cfg(feature = "read-write")]
use crate::{connection::WriteContext, error::OrmError};

const MAX_OUTBOX_BATCH: u64 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageInterest {
    Favorite,
    Downloaded,
    FavoriteAndDownloaded,
}

impl PackageInterest {
    #[must_use]
    pub const fn reason(self) -> &'static str {
        match self {
            Self::Favorite => "you favorited this package",
            Self::Downloaded => "you downloaded this package",
            Self::FavoriteAndDownloaded => "you favorited and downloaded this package",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SecuritySeverity {
    Moderate,
    High,
    Critical,
}

impl SecuritySeverity {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Moderate => "moderate",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "moderate" => Some(Self::Moderate),
            "high" => Some(Self::High),
            "critical" => Some(Self::Critical),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DigestFrequency {
    Daily,
    Weekly,
    Never,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationRecipient {
    pub user_id: Uuid,
    pub email: String,
    pub display_name: Option<String>,
    pub interest: PackageInterest,
    pub major_release_email: bool,
    pub security_email: bool,
    pub digest_email: bool,
    pub minimum_security_severity: SecuritySeverity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedEmail {
    pub subject: String,
    pub text: String,
    pub html: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DigestItemKind {
    MajorRelease,
    SecurityPatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DigestItem {
    pub package_name: String,
    pub current_version: String,
    pub latest_version: String,
    pub package_url: String,
    pub kind: DigestItemKind,
    pub interest: PackageInterest,
    pub security_severity: Option<SecuritySeverity>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct QueueEmailInput {
    pub user_id: Uuid,
    pub package_id: Option<Uuid>,
    pub event_type: &'static str,
    pub idempotency_key: String,
    pub recipient_email: String,
    pub recipient_name: Option<String>,
    pub rendered: RenderedEmail,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OutboxEmail {
    pub id: Uuid,
    pub user_id: Uuid,
    pub package_id: Option<Uuid>,
    pub event_type: String,
    pub idempotency_key: String,
    pub recipient_email: String,
    pub recipient_name: Option<String>,
    pub subject: String,
    pub text_body: String,
    pub html_body: String,
    pub attempt_count: i32,
    pub metadata: serde_json::Value,
}

/// Major-release notifications are only emitted for semantic versions whose
/// numeric major component increases. CalVer and opaque schemes should use a
/// different product rule rather than guessing that their first component has
/// SemVer meaning.
#[must_use]
pub fn is_major_semver_bump(previous: &str, current: &str) -> bool {
    match (semver_major(previous), semver_major(current)) {
        (Some(previous), Some(current)) => current > previous,
        _ => false,
    }
}

fn semver_major(value: &str) -> Option<u64> {
    let core = value.split(['-', '+']).next()?;
    let mut pieces = core.split('.');
    let major = pieces.next()?.parse::<u64>().ok()?;
    pieces.next()?.parse::<u64>().ok()?;
    pieces.next()?.parse::<u64>().ok()?;
    if pieces.next().is_some() {
        return None;
    }
    Some(major)
}

#[must_use]
pub fn security_meets_threshold(severity: SecuritySeverity, minimum: SecuritySeverity) -> bool {
    severity >= minimum
}

#[must_use]
pub fn render_major_release_email(
    package_name: &str,
    previous_version: &str,
    new_version: &str,
    package_url: &str,
    release_notes: Option<&str>,
    interest: PackageInterest,
    preferences_url: &str,
    unsubscribe_url: &str,
) -> RenderedEmail {
    let subject = format!("Zed: {package_name} {new_version} is a new major release");
    let notes_text = release_notes
        .filter(|value| !value.trim().is_empty())
        .map(|value| format!("\n\nRelease notes:\n{}", value.trim()))
        .unwrap_or_default();
    let text = format!(
        "{package_name} moved from {previous_version} to {new_version}. You are receiving this because {}.{notes_text}\n\nPackage: {package_url}\nNotification settings: {preferences_url}\nUnsubscribe from Zed package email: {unsubscribe_url}",
        interest.reason()
    );
    let notes_html = release_notes
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            format!(
                "<p><strong>Release notes:</strong><br>{}</p>",
                escape_html(value.trim())
            )
        })
        .unwrap_or_default();
    let body = format!(
        "<p><strong>{}</strong> moved from <code>{}</code> to <code>{}</code>.</p><p>You are receiving this because {}.</p>{notes_html}<p><a href=\"{}\">View package</a></p>",
        escape_html(package_name),
        escape_html(previous_version),
        escape_html(new_version),
        interest.reason(),
        escape_html(package_url)
    );
    RenderedEmail {
        subject,
        text,
        html: email_shell(
            &format!("New major release: {}", escape_html(package_name)),
            &body,
            preferences_url,
            unsubscribe_url,
        ),
    }
}

#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn render_security_patch_email(
    package_name: &str,
    affected_version: &str,
    fixed_version: &str,
    package_url: &str,
    severity: SecuritySeverity,
    advisory_id: &str,
    advisory_url: &str,
    summary: Option<&str>,
    interest: PackageInterest,
    preferences_url: &str,
    unsubscribe_url: &str,
) -> RenderedEmail {
    let subject = format!(
        "Zed security update: {package_name} {fixed_version} ({})",
        severity.as_str()
    );
    let summary_text = summary
        .filter(|value| !value.trim().is_empty())
        .map(|value| format!("\n\n{}", value.trim()))
        .unwrap_or_default();
    let text = format!(
        "A {} security issue affects {package_name} {affected_version}. The fix is available in {fixed_version}. Advisory: {advisory_id}.{summary_text}\n\nYou are receiving this because {}.\nPackage: {package_url}\nAdvisory: {advisory_url}\nNotification settings: {preferences_url}\nUnsubscribe from Zed package email: {unsubscribe_url}",
        severity.as_str(),
        interest.reason()
    );
    let summary_html = summary
        .filter(|value| !value.trim().is_empty())
        .map(|value| format!("<p>{}</p>", escape_html(value.trim())))
        .unwrap_or_default();
    let body = format!(
        "<p>A <strong>{}</strong> security issue affects <strong>{}</strong> <code>{}</code>. The fix is available in <code>{}</code>.</p><p>Advisory: <a href=\"{}\">{}</a></p>{summary_html}<p>You are receiving this because {}.</p><p><a href=\"{}\">View package</a></p>",
        severity.as_str(),
        escape_html(package_name),
        escape_html(affected_version),
        escape_html(fixed_version),
        escape_html(advisory_url),
        escape_html(advisory_id),
        interest.reason(),
        escape_html(package_url)
    );
    RenderedEmail {
        subject,
        text,
        html: email_shell(
            &format!("Security update: {}", escape_html(package_name)),
            &body,
            preferences_url,
            unsubscribe_url,
        ),
    }
}

#[must_use]
pub fn render_digest_email(
    period_start: &str,
    period_end: &str,
    items: &[DigestItem],
    preferences_url: &str,
    unsubscribe_url: &str,
) -> Option<RenderedEmail> {
    if items.is_empty() {
        return None;
    }
    let security_count = items
        .iter()
        .filter(|item| item.kind == DigestItemKind::SecurityPatch)
        .count();
    let subject = if security_count == 0 {
        format!("Zed package digest: {} updates", items.len())
    } else {
        format!(
            "Zed package digest: {} updates, {} security",
            items.len(),
            security_count
        )
    };
    let mut text = format!("Zed package digest for {period_start} through {period_end}\n");
    let mut html_items = String::new();
    for item in items {
        let kind = match item.kind {
            DigestItemKind::MajorRelease => "major release".to_owned(),
            DigestItemKind::SecurityPatch => format!(
                "{} security patch",
                item.security_severity
                    .map(SecuritySeverity::as_str)
                    .unwrap_or("security")
            ),
        };
        text.push_str(&format!(
            "\n- {}: {} -> {} ({kind}); {}\n  {}",
            item.package_name,
            item.current_version,
            item.latest_version,
            item.interest.reason(),
            item.package_url
        ));
        html_items.push_str(&format!(
            "<li><a href=\"{}\"><strong>{}</strong></a>: <code>{}</code> → <code>{}</code> — {} ({})</li>",
            escape_html(&item.package_url),
            escape_html(&item.package_name),
            escape_html(&item.current_version),
            escape_html(&item.latest_version),
            escape_html(&kind),
            item.interest.reason()
        ));
    }
    text.push_str(&format!(
        "\n\nNotification settings: {preferences_url}\nUnsubscribe from Zed package email: {unsubscribe_url}"
    ));
    Some(RenderedEmail {
        subject,
        text,
        html: email_shell(
            "Your Zed package digest",
            &format!(
                "<p>Updates from <strong>{}</strong> through <strong>{}</strong>.</p><ul>{html_items}</ul>",
                escape_html(period_start),
                escape_html(period_end)
            ),
            preferences_url,
            unsubscribe_url,
        ),
    })
}

fn email_shell(title: &str, body: &str, preferences_url: &str, unsubscribe_url: &str) -> String {
    format!(
        "<!doctype html><html><body><main><h1>{title}</h1>{body}<hr><p><a href=\"{}\">Notification settings</a> · <a href=\"{}\">Unsubscribe from package email</a></p></main></body></html>",
        escape_html(preferences_url),
        escape_html(unsubscribe_url)
    )
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(feature = "read-write")]
pub async fn load_package_notification_recipients(
    context: &WriteContext,
    package_id: Uuid,
) -> Result<Vec<NotificationRecipient>, OrmError> {
    let rows = context
        .connection()
        .query_all(Statement::from_sql_and_values(
            context.connection().get_database_backend(),
            r#"
            with interests as (
              select user_id,
                     bool_or(favorited) as favorited,
                     bool_or(downloaded) as downloaded
                from (
                  select user_id, true as favorited, false as downloaded
                    from zed_package_favorites
                   where package_id = $1
                  union all
                  select distinct downloaded_by_user_id as user_id,
                         false as favorited,
                         true as downloaded
                    from zed_package_downloads
                   where package_id = $1
                     and downloaded_by_user_id is not null
                ) source
               group by user_id
            )
            select u.id as user_id,
                   u.email as email,
                   u.display_name as display_name,
                   interests.favorited as favorited,
                   interests.downloaded as downloaded,
                   coalesce(p.major_release_email, true) as major_release_email,
                   coalesce(p.security_email, true) as security_email,
                   coalesce(p.digest_email, true) as digest_email,
                   coalesce(p.minimum_security_severity, 'high') as minimum_security_severity
              from interests
              join zed_users u on u.id = interests.user_id
              left join zed_notification_preferences p on p.user_id = u.id
             where u.email is not null
               and u.is_soft_deleted = false
               and coalesce(p.email_enabled, true) = true
               and p.unsubscribed_at is null
               and ((interests.favorited and coalesce(p.favorite_package_email, true))
                 or (interests.downloaded and coalesce(p.downloaded_package_email, true)))
            "#,
            [package_id.into()],
        ))
        .await
        .map_err(OrmError::from_db_err)?;

    rows.into_iter()
        .map(|row| {
            let favorited = row
                .try_get::<bool>("", "favorited")
                .map_err(OrmError::from_db_err)?;
            let downloaded = row
                .try_get::<bool>("", "downloaded")
                .map_err(OrmError::from_db_err)?;
            let interest = match (favorited, downloaded) {
                (true, true) => PackageInterest::FavoriteAndDownloaded,
                (true, false) => PackageInterest::Favorite,
                (false, true) => PackageInterest::Downloaded,
                (false, false) => {
                    return Err(OrmError::policy(
                        "notification recipient query returned no package interest",
                    ));
                }
            };
            let severity = row
                .try_get::<String>("", "minimum_security_severity")
                .map_err(OrmError::from_db_err)?;
            let minimum_security_severity =
                SecuritySeverity::parse(&severity).ok_or_else(|| {
                    OrmError::policy("notification recipient has invalid security threshold")
                })?;
            Ok(NotificationRecipient {
                user_id: row
                    .try_get::<Uuid>("", "user_id")
                    .map_err(OrmError::from_db_err)?,
                email: row
                    .try_get::<String>("", "email")
                    .map_err(OrmError::from_db_err)?,
                display_name: row
                    .try_get::<Option<String>>("", "display_name")
                    .map_err(OrmError::from_db_err)?,
                interest,
                major_release_email: row
                    .try_get::<bool>("", "major_release_email")
                    .map_err(OrmError::from_db_err)?,
                security_email: row
                    .try_get::<bool>("", "security_email")
                    .map_err(OrmError::from_db_err)?,
                digest_email: row
                    .try_get::<bool>("", "digest_email")
                    .map_err(OrmError::from_db_err)?,
                minimum_security_severity,
            })
        })
        .collect()
}

#[cfg(feature = "read-write")]
pub async fn queue_rendered_email(
    context: &WriteContext,
    input: QueueEmailInput,
) -> Result<bool, OrmError> {
    if !matches!(
        input.event_type,
        "major_release" | "security_patch" | "digest"
    ) {
        return Err(OrmError::policy("unsupported notification event type"));
    }
    let result = context
        .connection()
        .execute(Statement::from_sql_and_values(
            context.connection().get_database_backend(),
            r#"
            insert into zed_email_notification_outbox (
              id, user_id, package_id, event_type, idempotency_key,
              recipient_email, recipient_name, subject, text_body, html_body,
              metadata
            ) values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
            on conflict (idempotency_key) do nothing
            "#,
            [
                Uuid::new_v4().into(),
                input.user_id.into(),
                input.package_id.into(),
                input.event_type.into(),
                input.idempotency_key.into(),
                input.recipient_email.into(),
                input.recipient_name.into(),
                input.rendered.subject.into(),
                input.rendered.text.into(),
                input.rendered.html.into(),
                input.metadata.into(),
            ],
        ))
        .await
        .map_err(OrmError::from_db_err)?;
    Ok(result.rows_affected() == 1)
}

/// Atomically leases a batch using `FOR UPDATE SKIP LOCKED`; multiple Zed
/// dispatcher replicas can therefore drain one outbox without double-claiming
/// rows. A crashed dispatcher can be recovered by a separate stale-lease reset
/// because `available_at` remains the scheduling authority.
#[cfg(feature = "read-write")]
pub async fn claim_outbox_batch(
    context: &WriteContext,
    limit: u64,
) -> Result<Vec<OutboxEmail>, OrmError> {
    let limit = limit.clamp(1, MAX_OUTBOX_BATCH);
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
                   attempt_count = attempt_count + 1
              from claimed
             where outbox.id = claimed.id
            returning outbox.id, outbox.user_id, outbox.package_id,
                      outbox.event_type, outbox.idempotency_key,
                      outbox.recipient_email, outbox.recipient_name,
                      outbox.subject, outbox.text_body, outbox.html_body,
                      outbox.attempt_count, outbox.metadata
            "#,
            [(limit as i64).into()],
        ))
        .await
        .map_err(OrmError::from_db_err)?;

    rows.into_iter()
        .map(|row| {
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
        })
        .collect()
}

#[cfg(feature = "read-write")]
pub async fn mark_outbox_published(context: &WriteContext, id: Uuid) -> Result<(), OrmError> {
    context
        .connection()
        .execute(Statement::from_sql_and_values(
            context.connection().get_database_backend(),
            "update zed_email_notification_outbox set state = 'published', published_at = now(), last_error_code = null where id = $1 and state = 'publishing'",
            [id.into()],
        ))
        .await
        .map_err(OrmError::from_db_err)?;
    Ok(())
}

#[cfg(feature = "read-write")]
pub async fn release_outbox_for_retry(
    context: &WriteContext,
    id: Uuid,
    error_code: &str,
    retry_after_seconds: u64,
) -> Result<(), OrmError> {
    let retry_after_seconds = retry_after_seconds.clamp(1, 86_400) as i64;
    context
        .connection()
        .execute(Statement::from_sql_and_values(
            context.connection().get_database_backend(),
            "update zed_email_notification_outbox set state = 'pending', available_at = now() + ($2::bigint * interval '1 second'), last_error_code = left($3, 128) where id = $1 and state = 'publishing'",
            [id.into(), retry_after_seconds.into(), error_code.into()],
        ))
        .await
        .map_err(OrmError::from_db_err)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_only_semver_major_increases() {
        assert!(is_major_semver_bump("1.9.9", "2.0.0"));
        assert!(is_major_semver_bump("1.9.9", "3.0.0-beta.1"));
        assert!(!is_major_semver_bump("2.0.0", "2.1.0"));
        assert!(!is_major_semver_bump("2026.09", "2027.01"));
        assert!(!is_major_semver_bump("opaque", "2.0.0"));
    }

    #[test]
    fn security_threshold_order_is_explicit() {
        assert!(security_meets_threshold(
            SecuritySeverity::Critical,
            SecuritySeverity::High
        ));
        assert!(security_meets_threshold(
            SecuritySeverity::High,
            SecuritySeverity::High
        ));
        assert!(!security_meets_threshold(
            SecuritySeverity::Moderate,
            SecuritySeverity::High
        ));
    }

    #[test]
    fn rendering_escapes_untrusted_package_and_release_text() {
        let email = render_major_release_email(
            "<script>alert(1)</script>",
            "1.0.0",
            "2.0.0",
            "https://zed.pkg/p/org/pkg",
            Some("<b>breaking</b>"),
            PackageInterest::Favorite,
            "https://zed.pkg/settings/notifications",
            "https://zed.pkg/unsubscribe/token",
        );
        assert!(!email.html.contains("<script>"));
        assert!(email.html.contains("&lt;script&gt;"));
        assert!(email.html.contains("&lt;b&gt;breaking&lt;/b&gt;"));
        assert!(email.subject.contains("2.0.0"));
    }

    #[test]
    fn empty_digest_is_not_rendered() {
        assert!(render_digest_email(
            "2026-09-01",
            "2026-09-08",
            &[],
            "https://zed.pkg/settings/notifications",
            "https://zed.pkg/unsubscribe/token",
        )
        .is_none());
    }
}
