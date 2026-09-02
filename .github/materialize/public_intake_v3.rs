//! Encrypted persistence boundary for public commercial-intake submissions.
//!
//! The Cloudflare edge validates abuse proofs and signs normalized requests, while
//! the API validates the shared transport contract. This module owns only the
//! durable database boundary. It stores no plaintext email, JSON payload, raw
//! body digest, IP address, or abuse-challenge proof.

use std::fmt;

use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait, TryGetable,
};
use sha2::Sha256;
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

use zed_interfaces::PublicIntakeSourceHostV1;

pub const PUBLIC_INTAKE_ENCRYPTION_FORMAT_V1: &str = "aes-256-gcm-v1";
pub const PUBLIC_INTAKE_FINGERPRINT_FORMAT_V1: &str = "hmac-sha256-v1";

const ENCRYPTION_KEY_BYTES: usize = 32;
const SHA256_BYTES: usize = 32;
const GCM_NONCE_BYTES: usize = 12;
const MIN_LOOKUP_KEY_BYTES: usize = 32;
const MIN_RETENTION_DAYS: i64 = 1;
const MAX_RETENTION_DAYS: i64 = 3650;
const MAX_PURGE_BATCH: u64 = 10_000;
const EMAIL_HMAC_DOMAIN_V1: &[u8] = b"zpkg-public-intake-email-v1\n";
const BODY_HMAC_DOMAIN_V1: &[u8] = b"zpkg-public-intake-body-v1\n";
const EMAIL_AAD_V1: &str = "zpkg-public-intake-email-v1";
const PAYLOAD_AAD_V1: &str = "zpkg-public-intake-payload-v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicIntakeSubmissionKind {
    PreInterest,
    QuoteRequest,
}

impl PublicIntakeSubmissionKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::PreInterest => "pre_interest",
            Self::QuoteRequest => "quote_request",
        }
    }
}

/// Owned secret bytes that are erased before their allocation is released.
pub struct SecretBytes(Vec<u8>);

impl SecretBytes {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl Drop for SecretBytes {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

pub struct NewPublicIntakeSubmission {
    request_id: Uuid,
    kind: PublicIntakeSubmissionKind,
    source_host: &'static str,
    body_fingerprint_hmac: [u8; SHA256_BYTES],
    encryption_key_id: String,
    email_hmac_key_id: String,
    email_lookup_hmac: [u8; SHA256_BYTES],
    normalized_email_ciphertext: Vec<u8>,
    normalized_payload_ciphertext: Vec<u8>,
    consented_at: DateTime<Utc>,
    marketing_consent: bool,
    retention_expires_at: DateTime<Utc>,
}

/// Debug output is intentionally useful for control-flow diagnosis while never
/// serializing contact data, ciphertext, digests, or keyed lookup material.
impl fmt::Debug for NewPublicIntakeSubmission {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NewPublicIntakeSubmission")
            .field("request_id", &self.request_id)
            .field("kind", &self.kind)
            .field("source_host", &self.source_host)
            .field("encryption_format", &PUBLIC_INTAKE_ENCRYPTION_FORMAT_V1)
            .field("fingerprint_format", &PUBLIC_INTAKE_FINGERPRINT_FORMAT_V1)
            .field("body_fingerprint_hmac", &"[REDACTED]")
            .field("encryption_key_id", &self.encryption_key_id)
            .field("email_hmac_key_id", &self.email_hmac_key_id)
            .field("email_lookup_hmac", &"[REDACTED]")
            .field("normalized_email_ciphertext", &"[REDACTED]")
            .field("normalized_payload_ciphertext", &"[REDACTED]")
            .field("consented_at", &self.consented_at)
            .field("marketing_consent", &self.marketing_consent)
            .field("retention_expires_at", &self.retention_expires_at)
            .finish()
    }
}

impl NewPublicIntakeSubmission {
    #[allow(clippy::too_many_arguments)]
    pub fn encrypted(
        kind: PublicIntakeSubmissionKind,
        source_host: PublicIntakeSourceHostV1,
        request_id: &str,
        body_sha256_hex: &str,
        normalized_email: &str,
        normalized_payload: &[u8],
        consented_at: DateTime<Utc>,
        marketing_consent: bool,
        retention_expires_at: DateTime<Utc>,
        encryption_key_id: &str,
        encryption_key: &SecretBytes,
        email_hmac_key_id: &str,
        email_hmac_key: &SecretBytes,
    ) -> Result<Self, PublicIntakeStoreError> {
        let request_id = Uuid::parse_str(request_id)
            .map_err(|_| PublicIntakeStoreError::InvalidRequestId)?;
        if body_sha256_hex.len() != SHA256_BYTES * 2
            || !body_sha256_hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err(PublicIntakeStoreError::InvalidBodyDigest);
        }
        let body_sha256 =
            Zeroizing::new(hex::decode(body_sha256_hex).map_err(|_| PublicIntakeStoreError::InvalidBodyDigest)?);
        if body_sha256.len() != SHA256_BYTES {
            return Err(PublicIntakeStoreError::InvalidBodyDigest);
        }

        let normalized_email = Zeroizing::new(normalized_email.trim().to_ascii_lowercase());
        if normalized_email.is_empty()
            || normalized_email.len() > 254
            || normalized_email.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(PublicIntakeStoreError::InvalidNormalizedEmail);
        }
        if !portable_key_id(encryption_key_id) || !portable_key_id(email_hmac_key_id) {
            return Err(PublicIntakeStoreError::InvalidKeyId);
        }
        if encryption_key.expose().len() != ENCRYPTION_KEY_BYTES {
            return Err(PublicIntakeStoreError::InvalidEncryptionKey);
        }
        if email_hmac_key.expose().len() < MIN_LOOKUP_KEY_BYTES {
            return Err(PublicIntakeStoreError::InvalidLookupKey);
        }
        let retention_days = retention_expires_at
            .signed_duration_since(consented_at)
            .num_days();
        if !(MIN_RETENTION_DAYS..=MAX_RETENTION_DAYS).contains(&retention_days) {
            return Err(PublicIntakeStoreError::InvalidRetentionExpiry);
        }

        let source_host = match source_host {
            PublicIntakeSourceHostV1::User => "user.zpkg.net",
            PublicIntakeSourceHostV1::Organization => "org.zpkg.net",
        };
        if kind == PublicIntakeSubmissionKind::QuoteRequest && source_host != "org.zpkg.net" {
            return Err(PublicIntakeStoreError::InvalidKindHostPair);
        }

        let email_lookup_hmac = keyed_fingerprint(
            email_hmac_key.expose(),
            EMAIL_HMAC_DOMAIN_V1,
            &[normalized_email.as_bytes()],
        )?;
        let body_fingerprint_hmac = keyed_fingerprint(
            email_hmac_key.expose(),
            BODY_HMAC_DOMAIN_V1,
            &[request_id.as_bytes(), body_sha256.as_slice()],
        )?;

        let email_aad = format!(
            "{EMAIL_AAD_V1}\n{request_id}\n{encryption_key_id}\n{email_hmac_key_id}"
        );
        let payload_aad = format!(
            "{PAYLOAD_AAD_V1}\n{request_id}\n{}\n{source_host}\n{encryption_key_id}",
            kind.as_str()
        );
        let normalized_email_ciphertext = encrypt(
            encryption_key.expose(),
            normalized_email.as_bytes(),
            email_aad.as_bytes(),
        )?;
        let normalized_payload_ciphertext = encrypt(
            encryption_key.expose(),
            normalized_payload,
            payload_aad.as_bytes(),
        )?;

        Ok(Self {
            request_id,
            kind,
            source_host,
            body_fingerprint_hmac,
            encryption_key_id: encryption_key_id.to_owned(),
            email_hmac_key_id: email_hmac_key_id.to_owned(),
            email_lookup_hmac,
            normalized_email_ciphertext,
            normalized_payload_ciphertext,
            consented_at,
            marketing_consent,
            retention_expires_at,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PublicIntakeInsertResult {
    pub request_id: Uuid,
    pub inserted: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicIntakeStoreError {
    Database,
    EncryptionFailed,
    IdempotencyConflict,
    InvalidBodyDigest,
    InvalidEncryptionKey,
    InvalidKeyId,
    InvalidKindHostPair,
    InvalidLookupKey,
    InvalidNormalizedEmail,
    InvalidPurgeBatch,
    InvalidRequestId,
    InvalidRetentionExpiry,
}

impl fmt::Display for PublicIntakeStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Database => "public intake database operation failed",
            Self::EncryptionFailed => "public intake encryption failed",
            Self::IdempotencyConflict => "public intake idempotency conflict",
            Self::InvalidBodyDigest => "public intake body digest is invalid",
            Self::InvalidEncryptionKey => "public intake encryption key is invalid",
            Self::InvalidKeyId => "public intake key identifier is invalid",
            Self::InvalidKindHostPair => "public intake kind and source host are inconsistent",
            Self::InvalidLookupKey => "public intake lookup key is invalid",
            Self::InvalidNormalizedEmail => "public intake normalized email is invalid",
            Self::InvalidPurgeBatch => "public intake purge batch is invalid",
            Self::InvalidRequestId => "public intake request id is invalid",
            Self::InvalidRetentionExpiry => "public intake retention expiry is invalid",
        })
    }
}

impl std::error::Error for PublicIntakeStoreError {}

impl PublicIntakeStoreError {
    fn from_db(error: impl fmt::Display) -> Self {
        let _ = error;
        Self::Database
    }
}

pub(crate) async fn insert_public_intake_submission_on_database(
    database: &DatabaseConnection,
    input: NewPublicIntakeSubmission,
) -> Result<PublicIntakeInsertResult, PublicIntakeStoreError> {
    let request_id = input.request_id;
    let kind = input.kind.as_str().to_owned();
    let source_host = input.source_host.to_owned();
    let body_fingerprint_hmac = input.body_fingerprint_hmac.to_vec();

    let transaction = database
        .begin()
        .await
        .map_err(PublicIntakeStoreError::from_db)?;
    let inserted = transaction
        .query_one(Statement::from_sql_and_values(
            DbBackend::Postgres,
            r#"
            insert into zed_public_intake_submissions (
                request_id,
                kind,
                source_host,
                body_fingerprint_hmac,
                encryption_format,
                fingerprint_format,
                encryption_key_id,
                email_hmac_key_id,
                email_lookup_hmac,
                normalized_email_ciphertext,
                payload_ciphertext,
                consented_at,
                marketing_consent,
                retention_expires_at
            ) values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
            on conflict do nothing
            returning request_id
            "#,
            vec![
                request_id.into(),
                kind.clone().into(),
                source_host.clone().into(),
                body_fingerprint_hmac.clone().into(),
                PUBLIC_INTAKE_ENCRYPTION_FORMAT_V1.into(),
                PUBLIC_INTAKE_FINGERPRINT_FORMAT_V1.into(),
                input.encryption_key_id.into(),
                input.email_hmac_key_id.into(),
                input.email_lookup_hmac.to_vec().into(),
                input.normalized_email_ciphertext.into(),
                input.normalized_payload_ciphertext.into(),
                input.consented_at.into(),
                input.marketing_consent.into(),
                input.retention_expires_at.into(),
            ],
        ))
        .await
        .map_err(PublicIntakeStoreError::from_db)?
        .is_some();

    if inserted {
        transaction
            .commit()
            .await
            .map_err(PublicIntakeStoreError::from_db)?;
        return Ok(PublicIntakeInsertResult {
            request_id,
            inserted: true,
        });
    }

    let existing = transaction
        .query_one(Statement::from_sql_and_values(
            DbBackend::Postgres,
            r#"
            select body_fingerprint_hmac, kind, source_host
            from zed_public_intake_submissions
            where request_id = $1
            "#,
            vec![request_id.into()],
        ))
        .await
        .map_err(PublicIntakeStoreError::from_db)?;

    if let Some(existing) = existing {
        let existing_fingerprint: Vec<u8> = existing
            .try_get("", "body_fingerprint_hmac")
            .map_err(PublicIntakeStoreError::from_db)?;
        let existing_kind: String = existing
            .try_get("", "kind")
            .map_err(PublicIntakeStoreError::from_db)?;
        let existing_source_host: String = existing
            .try_get("", "source_host")
            .map_err(PublicIntakeStoreError::from_db)?;
        if existing_fingerprint != body_fingerprint_hmac
            || existing_kind != kind
            || existing_source_host != source_host
        {
            return Err(PublicIntakeStoreError::IdempotencyConflict);
        }
    }

    transaction
        .commit()
        .await
        .map_err(PublicIntakeStoreError::from_db)?;
    Ok(PublicIntakeInsertResult {
        request_id,
        inserted: false,
    })
}

pub(crate) async fn purge_expired_public_intake_submissions_on_database(
    database: &DatabaseConnection,
    cutoff: DateTime<Utc>,
    batch_limit: u64,
) -> Result<u64, PublicIntakeStoreError> {
    if batch_limit == 0 || batch_limit > MAX_PURGE_BATCH {
        return Err(PublicIntakeStoreError::InvalidPurgeBatch);
    }
    let rows = database
        .query_all(Statement::from_sql_and_values(
            DbBackend::Postgres,
            r#"
            with expired as (
                select request_id
                from zed_public_intake_submissions
                where retention_expires_at <= $1
                order by retention_expires_at, request_id
                limit $2
                for update skip locked
            )
            delete from zed_public_intake_submissions as submissions
            using expired
            where submissions.request_id = expired.request_id
            returning submissions.request_id
            "#,
            vec![cutoff.into(), batch_limit.into()],
        ))
        .await
        .map_err(PublicIntakeStoreError::from_db)?;
    Ok(rows.len() as u64)
}

fn keyed_fingerprint(
    key: &[u8],
    domain: &[u8],
    values: &[&[u8]],
) -> Result<[u8; SHA256_BYTES], PublicIntakeStoreError> {
    let mut hmac = <Hmac<Sha256> as Mac>::new_from_slice(key)
        .map_err(|_| PublicIntakeStoreError::InvalidLookupKey)?;
    hmac.update(domain);
    for value in values {
        hmac.update(&(value.len() as u64).to_be_bytes());
        hmac.update(value);
    }
    Ok(hmac.finalize().into_bytes().into())
}

fn encrypt(key: &[u8], plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>, PublicIntakeStoreError> {
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|_| PublicIntakeStoreError::InvalidEncryptionKey)?;
    let nonce: [u8; GCM_NONCE_BYTES] = rand::random();
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce), Payload { msg: plaintext, aad })
        .map_err(|_| PublicIntakeStoreError::EncryptionFailed)?;
    let mut envelope = Vec::with_capacity(GCM_NONCE_BYTES + ciphertext.len());
    envelope.extend_from_slice(&nonce);
    envelope.extend_from_slice(&ciphertext);
    Ok(envelope)
}

fn portable_key_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.is_ascii()
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index > 0 && matches!(byte, b'.' | b'_' | b'-'))
        })
}

#[cfg(test)]
mod tests {
    use chrono::Duration;
    use sea_orm::{Database, DatabaseConnection};

    use super::*;

    fn secret(byte: u8) -> SecretBytes {
        SecretBytes::new(vec![byte; 32])
    }

    fn consented_at() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-09-02T15:00:00Z")
            .expect("fixed timestamp")
            .with_timezone(&Utc)
    }

    fn test_submission(
        request_id: &str,
        digest_byte: &str,
        email: &str,
        payload: &[u8],
    ) -> NewPublicIntakeSubmission {
        let consented_at = consented_at();
        NewPublicIntakeSubmission::encrypted(
            PublicIntakeSubmissionKind::PreInterest,
            PublicIntakeSourceHostV1::User,
            request_id,
            &digest_byte.repeat(32),
            email,
            payload,
            consented_at,
            false,
            consented_at + Duration::days(365),
            "enc-2026-09",
            &secret(7),
            "lookup-2026-09",
            &secret(9),
        )
        .expect("valid encrypted submission")
    }

    #[test]
    fn encrypted_submission_never_formats_contact_or_payload_data() {
        let submission = test_submission(
            "018f5f52-feb8-7d4a-a9d6-69d8a1559e8b",
            "ab",
            "Private.Person@Example.COM",
            br#"{"email":"private.person@example.com","note":"private requirement"}"#,
        );
        let debug = format!("{submission:?}");
        assert!(!debug.contains("Private.Person"));
        assert!(!debug.contains("private.person"));
        assert!(!debug.contains("private requirement"));
        assert!(debug.contains("[REDACTED]"));
    }

    #[test]
    fn raw_digest_and_email_fingerprints_are_domain_separated() {
        let key = vec![9; 32];
        let request_id = Uuid::parse_str("018f5f52-feb8-7d4a-a9d6-69d8a1559e8b").expect("uuid");
        let digest = vec![0xab; 32];
        let email = keyed_fingerprint(&key, EMAIL_HMAC_DOMAIN_V1, &[b"person@example.com"])
            .expect("email fingerprint");
        let body = keyed_fingerprint(
            &key,
            BODY_HMAC_DOMAIN_V1,
            &[request_id.as_bytes(), &digest],
        )
        .expect("body fingerprint");
        assert_ne!(email, body);
        assert_ne!(body.as_slice(), digest.as_slice());
    }

    #[test]
    fn retention_is_bounded() {
        let base = consented_at();
        for (expiry, expected) in [
            (base, Err(PublicIntakeStoreError::InvalidRetentionExpiry)),
            (base + Duration::hours(23), Err(PublicIntakeStoreError::InvalidRetentionExpiry)),
            (base + Duration::days(1), Ok(())),
            (base + Duration::days(3650), Ok(())),
            (base + Duration::days(3651), Err(PublicIntakeStoreError::InvalidRetentionExpiry)),
        ] {
            let result = NewPublicIntakeSubmission::encrypted(
                PublicIntakeSubmissionKind::PreInterest,
                PublicIntakeSourceHostV1::User,
                "018f5f52-feb8-7d4a-a9d6-69d8a1559e8b",
                &"ab".repeat(32),
                "person@example.com",
                b"{}",
                base,
                false,
                expiry,
                "enc-2026-09",
                &secret(7),
                "lookup-2026-09",
                &secret(9),
            )
            .map(|_| ());
            assert_eq!(result, expected);
        }
    }

    #[test]
    fn quote_requests_are_bound_to_the_organization_host() {
        let base = consented_at();
        let error = NewPublicIntakeSubmission::encrypted(
            PublicIntakeSubmissionKind::QuoteRequest,
            PublicIntakeSourceHostV1::User,
            "018f5f52-feb8-7d4a-a9d6-69d8a1559e8b",
            &"ab".repeat(32),
            "person@example.com",
            b"{}",
            base,
            false,
            base + Duration::days(365),
            "enc-2026-09",
            &secret(7),
            "lookup-2026-09",
            &secret(9),
        )
        .expect_err("quote requests must use org.zpkg.net");
        assert_eq!(error, PublicIntakeStoreError::InvalidKindHostPair);
    }

    #[test]
    fn database_errors_are_redacted() {
        let error = PublicIntakeStoreError::from_db("email=private.person@example.com");
        assert_eq!(error.to_string(), "public intake database operation failed");
        assert!(!format!("{error:?}").contains("private.person"));
    }

    #[test]
    fn schema_marker_matches_transport_authority() {
        assert_eq!(
            zed_interfaces::PUBLIC_INTAKE_SCHEMA_V1,
            "zed.public-intake.v1"
        );
    }

    async fn postgres_database() -> Option<DatabaseConnection> {
        let required = std::env::var("PUBLIC_INTAKE_REQUIRE_POSTGRES")
            .is_ok_and(|value| value == "1");
        let Ok(url) = std::env::var("PUBLIC_INTAKE_TEST_DATABASE_URL") else {
            assert!(!required, "PUBLIC_INTAKE_TEST_DATABASE_URL is required");
            return None;
        };
        Some(Database::connect(url).await.expect("connect test PostgreSQL"))
    }

    #[tokio::test]
    async fn postgres_enforces_encryption_idempotency_and_retention() {
        let Some(database) = postgres_database().await else {
            return;
        };
        database
            .execute_unprepared(include_str!("sql/2026-09-02-public-intake.sql"))
            .await
            .expect("apply public-intake migration");
        database
            .execute_unprepared("delete from zed_public_intake_submissions")
            .await
            .expect("isolate test table");

        let request_id = "018f5f52-feb8-7d4a-a9d6-69d8a1559e8b";
        let first = insert_public_intake_submission_on_database(
            &database,
            test_submission(
                request_id,
                "ab",
                "private.person@example.com",
                br#"{"email":"private.person@example.com","kind":"first"}"#,
            ),
        )
        .await
        .expect("first insert");
        assert!(first.inserted);

        let replay = insert_public_intake_submission_on_database(
            &database,
            test_submission(
                request_id,
                "ab",
                "private.person@example.com",
                br#"{"email":"private.person@example.com","kind":"first"}"#,
            ),
        )
        .await
        .expect("idempotent replay");
        assert!(!replay.inserted);

        let conflict = insert_public_intake_submission_on_database(
            &database,
            test_submission(
                request_id,
                "cd",
                "private.person@example.com",
                br#"{"email":"private.person@example.com","kind":"changed"}"#,
            ),
        )
        .await
        .expect_err("request identifier reuse with different content must fail");
        assert_eq!(conflict, PublicIntakeStoreError::IdempotencyConflict);

        let duplicate = insert_public_intake_submission_on_database(
            &database,
            test_submission(
                "018f5f52-feb8-7d4a-a9d6-69d8a1559e8c",
                "ef",
                "PRIVATE.PERSON@example.com",
                br#"{"email":"private.person@example.com","kind":"duplicate"}"#,
            ),
        )
        .await
        .expect("duplicate email remains enumeration-resistant");
        assert!(!duplicate.inserted);

        let row = database
            .query_one(Statement::from_sql_and_values(
                DbBackend::Postgres,
                "select body_fingerprint_hmac, encryption_format, fingerprint_format, normalized_email_ciphertext, payload_ciphertext from zed_public_intake_submissions where request_id = $1",
                vec![Uuid::parse_str(request_id).expect("uuid").into()],
            ))
            .await
            .expect("read encrypted row")
            .expect("encrypted row exists");
        let body_fingerprint: Vec<u8> = row.try_get("", "body_fingerprint_hmac").expect("body fingerprint");
        let encryption_format: String = row.try_get("", "encryption_format").expect("encryption format");
        let fingerprint_format: String = row.try_get("", "fingerprint_format").expect("fingerprint format");
        let email_ciphertext: Vec<u8> = row.try_get("", "normalized_email_ciphertext").expect("email ciphertext");
        let payload_ciphertext: Vec<u8> = row.try_get("", "payload_ciphertext").expect("payload ciphertext");
        assert_eq!(encryption_format, PUBLIC_INTAKE_ENCRYPTION_FORMAT_V1);
        assert_eq!(fingerprint_format, PUBLIC_INTAKE_FINGERPRINT_FORMAT_V1);
        assert_ne!(body_fingerprint, vec![0xab; 32]);
        for (ciphertext, plaintext) in [
            (&email_ciphertext, b"private.person@example.com".as_slice()),
            (&payload_ciphertext, b"private.person@example.com".as_slice()),
        ] {
            assert!(!ciphertext.windows(plaintext.len()).any(|window| window == plaintext));
        }

        let before_expiry = purge_expired_public_intake_submissions_on_database(
            &database,
            consented_at() + Duration::days(364),
            100,
        )
        .await
        .expect("pre-expiry purge");
        assert_eq!(before_expiry, 0);
        let after_expiry = purge_expired_public_intake_submissions_on_database(
            &database,
            consented_at() + Duration::days(366),
            100,
        )
        .await
        .expect("post-expiry purge");
        assert_eq!(after_expiry, 1);
    }
}
