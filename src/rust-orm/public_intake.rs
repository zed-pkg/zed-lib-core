//! Named write boundary for privacy-minimized public commercial intake.
//!
//! The API authenticates the Cloudflare edge, validates the shared DTO, and
//! computes the request and email fingerprints. This module owns the only SQL
//! write. Contact data and the complete normalized payload are encrypted by
//! PostgreSQL `pgcrypto`; the table contains no plaintext email, name,
//! organization, website, or requirements-summary columns.

use std::fmt;

use chrono::{DateTime, SecondsFormat, Utc};
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use uuid::Uuid;
use zed_interfaces::public_intake::PublicIntakeSourceHostV1;

use crate::{connection::WriteContext, error::OrmError};

const MAX_PAYLOAD_BYTES: usize = 16 * 1024;
const MIN_ENCRYPTION_KEY_BYTES: usize = 32;

/// Stable database discriminator. New variants require a forward migration;
/// existing spellings are part of the durable storage contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicIntakeKind {
    PreInterest,
    QuoteRequest,
}

impl PublicIntakeKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::PreInterest => "pre_interest",
            Self::QuoteRequest => "quote_request",
        }
    }

    const fn expected_host(self) -> PublicIntakeSourceHostV1 {
        match self {
            Self::PreInterest => PublicIntakeSourceHostV1::User,
            Self::QuoteRequest => PublicIntakeSourceHostV1::Organization,
        }
    }
}

/// Fully validated, normalized input for the persistence boundary.
///
/// Fingerprints are fixed-size bytes instead of caller-provided hexadecimal
/// strings, so malformed digest encodings cannot reach SQL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewPublicIntakeSubmission {
    request_id: Uuid,
    kind: PublicIntakeKind,
    source_host: PublicIntakeSourceHostV1,
    body_sha256: [u8; 32],
    email_lookup_hmac: [u8; 32],
    normalized_email: String,
    normalized_payload_json: String,
    consented_at: DateTime<Utc>,
    marketing_consent: bool,
}

impl NewPublicIntakeSubmission {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        request_id: Uuid,
        kind: PublicIntakeKind,
        source_host: PublicIntakeSourceHostV1,
        body_sha256: [u8; 32],
        email_lookup_hmac: [u8; 32],
        normalized_email: String,
        normalized_payload_json: String,
        consented_at: DateTime<Utc>,
        marketing_consent: bool,
    ) -> Result<Self, PublicIntakeWriteError> {
        if source_host != kind.expected_host() {
            return Err(PublicIntakeWriteError::InvalidInput(
                "intake kind does not match source host",
            ));
        }
        if !valid_normalized_email(&normalized_email) {
            return Err(PublicIntakeWriteError::InvalidInput(
                "normalized email is invalid",
            ));
        }
        if normalized_payload_json.is_empty()
            || normalized_payload_json.len() > MAX_PAYLOAD_BYTES
            || !serde_json::from_str::<serde_json::Value>(&normalized_payload_json)
                .is_ok_and(|value| value.is_object())
        {
            return Err(PublicIntakeWriteError::InvalidInput(
                "normalized payload is not a bounded JSON object",
            ));
        }

        Ok(Self {
            request_id,
            kind,
            source_host,
            body_sha256,
            email_lookup_hmac,
            normalized_email,
            normalized_payload_json,
            consented_at,
            marketing_consent,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicIntakeWriteOutcome {
    /// Covers both the first accepted write and a byte-identical replay. The
    /// public API deliberately does not expose which case occurred.
    Accepted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublicIntakeWriteError {
    InvalidInput(&'static str),
    IdempotencyConflict,
    Persistence(OrmError),
}

impl fmt::Display for PublicIntakeWriteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(message) => formatter.write_str(message),
            Self::IdempotencyConflict => {
                formatter.write_str("request id was already used for different content")
            }
            Self::Persistence(error) => write!(formatter, "public intake persistence failed: {error}"),
        }
    }
}

impl std::error::Error for PublicIntakeWriteError {}

impl From<OrmError> for PublicIntakeWriteError {
    fn from(error: OrmError) -> Self {
        Self::Persistence(error)
    }
}

/// Insert a new encrypted submission or accept an exact idempotent replay.
///
/// A reused request id with different body bytes, route kind, or source host
/// returns no row from the conditional conflict update and is rejected. The
/// comparison and replay counter update are one atomic PostgreSQL statement;
/// there is no read-before-write race.
pub async fn write_public_intake_submission(
    context: &WriteContext,
    submission: &NewPublicIntakeSubmission,
    encryption_key: &str,
) -> Result<PublicIntakeWriteOutcome, PublicIntakeWriteError> {
    if !valid_encryption_key(encryption_key) {
        return Err(PublicIntakeWriteError::InvalidInput(
            "public intake encryption key is invalid",
        ));
    }

    let connection = context.connection();
    if connection.get_database_backend() != DatabaseBackend::Postgres {
        return Err(PublicIntakeWriteError::Persistence(OrmError::policy(
            "public intake encryption requires PostgreSQL pgcrypto",
        )));
    }

    let source_host = match submission.source_host {
        PublicIntakeSourceHostV1::User => "user.zpkg.net",
        PublicIntakeSourceHostV1::Organization => "org.zpkg.net",
    };
    let consented_at = submission
        .consented_at
        .to_rfc3339_opts(SecondsFormat::Millis, true);

    let row = connection
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            INSERT_SQL,
            [
                submission.request_id.into(),
                submission.kind.as_str().into(),
                source_host.into(),
                submission.body_sha256.to_vec().into(),
                submission.email_lookup_hmac.to_vec().into(),
                submission.normalized_email.clone().into(),
                encryption_key.to_owned().into(),
                submission.normalized_payload_json.clone().into(),
                consented_at.into(),
                true.into(),
                submission.marketing_consent.into(),
            ],
        ))
        .await
        .map_err(OrmError::from_db_err)?;

    match row {
        Some(_) => Ok(PublicIntakeWriteOutcome::Accepted),
        None => Err(PublicIntakeWriteError::IdempotencyConflict),
    }
}

/// Exact runtime SQL. Every submitted value is bound. The encryption key is
/// supplied twice because pgcrypto encrypts the email and complete payload as
/// distinct ciphertexts; it is never interpolated into the statement.
const INSERT_SQL: &str = r#"
INSERT INTO zed_public_intake_submissions (
    request_id,
    kind,
    source_host,
    body_sha256,
    email_lookup_hmac,
    encrypted_email,
    encrypted_payload,
    consented_at,
    contact_consent,
    marketing_consent
) VALUES (
    $1,
    $2,
    $3,
    $4,
    $5,
    pgp_sym_encrypt($6, $7, 'cipher-algo=aes256,compress-algo=0'),
    pgp_sym_encrypt($8, $7, 'cipher-algo=aes256,compress-algo=0'),
    CAST($9 AS timestamptz),
    $10,
    $11
)
ON CONFLICT (request_id) DO UPDATE
SET
    last_seen_at = now(),
    replay_count = zed_public_intake_submissions.replay_count + 1
WHERE zed_public_intake_submissions.body_sha256 = EXCLUDED.body_sha256
  AND zed_public_intake_submissions.kind = EXCLUDED.kind
  AND zed_public_intake_submissions.source_host = EXCLUDED.source_host
RETURNING request_id
"#;

fn valid_normalized_email(value: &str) -> bool {
    if value.len() < 3
        || value.len() > 254
        || !value.is_ascii()
        || value.trim() != value
        || value.bytes().any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
        || value.bytes().any(|byte| byte.is_ascii_uppercase())
    {
        return false;
    }
    let mut parts = value.split('@');
    let local = parts.next().unwrap_or_default();
    let domain = parts.next().unwrap_or_default();
    if parts.next().is_some()
        || local.is_empty()
        || local.len() > 64
        || domain.is_empty()
        || domain.len() > 253
        || local.starts_with('.')
        || local.ends_with('.')
        || local.contains("..")
        || !domain.contains('.')
    {
        return false;
    }
    domain.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    })
}

fn valid_encryption_key(value: &str) -> bool {
    value.as_bytes().len() >= MIN_ENCRYPTION_KEY_BYTES
        && value.trim() == value
        && !value.bytes().any(|byte| matches!(byte, b'\r' | b'\n' | 0))
        && !value.to_ascii_uppercase().contains("PLACEHOLDER")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn submission() -> NewPublicIntakeSubmission {
        NewPublicIntakeSubmission::new(
            Uuid::parse_str("018f5f52-feb8-7d4a-a9d6-69d8a1559e8b").unwrap(),
            PublicIntakeKind::PreInterest,
            PublicIntakeSourceHostV1::User,
            [1; 32],
            [2; 32],
            "person@example.com".to_owned(),
            r#"{"schema":"zed.public-intake.v1","email":"person@example.com"}"#.to_owned(),
            DateTime::parse_from_rfc3339("2026-09-02T15:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            false,
        )
        .unwrap()
    }

    #[test]
    fn constructor_rejects_cross_host_intent_and_unbounded_payloads() {
        let mut candidate = submission();
        candidate.source_host = PublicIntakeSourceHostV1::Organization;
        assert!(matches!(
            NewPublicIntakeSubmission::new(
                candidate.request_id,
                candidate.kind,
                candidate.source_host,
                candidate.body_sha256,
                candidate.email_lookup_hmac,
                candidate.normalized_email,
                candidate.normalized_payload_json,
                candidate.consented_at,
                candidate.marketing_consent,
            ),
            Err(PublicIntakeWriteError::InvalidInput(_))
        ));

        let too_large = "x".repeat(MAX_PAYLOAD_BYTES + 1);
        assert!(NewPublicIntakeSubmission::new(
            Uuid::new_v4(),
            PublicIntakeKind::QuoteRequest,
            PublicIntakeSourceHostV1::Organization,
            [3; 32],
            [4; 32],
            "buyer@example.com".to_owned(),
            too_large,
            Utc::now(),
            true,
        )
        .is_err());
    }

    #[test]
    fn email_and_key_validation_are_fail_closed() {
        for email in [
            "Person@example.com",
            "person @example.com",
            "person@localhost",
            "person@example..com",
        ] {
            assert!(!valid_normalized_email(email), "{email}");
        }
        assert!(valid_normalized_email("person@example.com"));
        assert!(!valid_encryption_key("short"));
        assert!(!valid_encryption_key(
            "PLACEHOLDER-0123456789abcdef0123456789abcdef"
        ));
        assert!(valid_encryption_key(
            "0123456789abcdef0123456789abcdef"
        ));
    }

    #[test]
    fn runtime_sql_encrypts_contact_and_payload_and_has_atomic_replay_rules() {
        assert!(INSERT_SQL.contains("pgp_sym_encrypt($6, $7"));
        assert!(INSERT_SQL.contains("pgp_sym_encrypt($8, $7"));
        assert!(INSERT_SQL.contains("ON CONFLICT (request_id) DO UPDATE"));
        assert!(INSERT_SQL.contains("body_sha256 = EXCLUDED.body_sha256"));
        assert!(INSERT_SQL.contains("kind = EXCLUDED.kind"));
        assert!(INSERT_SQL.contains("source_host = EXCLUDED.source_host"));
        for forbidden in ["'person@example.com'", "format!(", "concat!("] {
            assert!(!INSERT_SQL.contains(forbidden));
        }
    }

    #[test]
    fn debug_output_never_contains_contact_or_payload_values() {
        let rendered = format!("{submission():?}");
        assert!(!rendered.contains("person@example.com"));
        assert!(!rendered.contains("zed.public-intake.v1"));
    }
}
