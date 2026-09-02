//! Encrypted persistence boundary for public commercial-intake submissions.
//!
//! The Cloudflare edge validates abuse proofs and signs normalized requests, while
//! the API validates the shared transport contract. This module owns only the
//! durable database boundary. It stores no plaintext email or JSON payload.

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
use zeroize::Zeroize;

use zed_interfaces::{PublicIntakeSourceHostV1, PUBLIC_INTAKE_SCHEMA_V1};

const ENCRYPTION_KEY_BYTES: usize = 32;
const SHA256_BYTES: usize = 32;
const GCM_NONCE_BYTES: usize = 12;
const MIN_LOOKUP_KEY_BYTES: usize = 32;
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
    body_sha256: [u8; SHA256_BYTES],
    email_lookup_hmac: [u8; SHA256_BYTES],
    normalized_email_ciphertext: Vec<u8>,
    normalized_payload_ciphertext: Vec<u8>,
    consented_at: DateTime<Utc>,
    marketing_consent: bool,
}

/// Debug output is intentionally useful for control-flow diagnosis while never
/// serializing contact data, ciphertext, digests, or lookup material.
impl fmt::Debug for NewPublicIntakeSubmission {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NewPublicIntakeSubmission")
            .field("request_id", &self.request_id)
            .field("kind", &self.kind)
            .field("source_host", &self.source_host)
            .field("body_sha256", &"[REDACTED]")
            .field("email_lookup_hmac", &"[REDACTED]")
            .field("normalized_email_ciphertext", &"[REDACTED]")
            .field("normalized_payload_ciphertext", &"[REDACTED]")
            .field("consented_at", &self.consented_at)
            .field("marketing_consent", &self.marketing_consent)
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
        encryption_key: &SecretBytes,
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
        let body_sha256_bytes =
            hex::decode(body_sha256_hex).map_err(|_| PublicIntakeStoreError::InvalidBodyDigest)?;
        let body_sha256: [u8; SHA256_BYTES] = body_sha256_bytes
            .try_into()
            .map_err(|_| PublicIntakeStoreError::InvalidBodyDigest)?;

        let normalized_email = normalized_email.trim().to_ascii_lowercase();
        if normalized_email.is_empty()
            || normalized_email.len() > 254
            || normalized_email.bytes().any(|byte| byte.is_ascii_control())
        {
            return Err(PublicIntakeStoreError::InvalidNormalizedEmail);
        }
        if encryption_key.expose().len() != ENCRYPTION_KEY_BYTES {
            return Err(PublicIntakeStoreError::InvalidEncryptionKey);
        }
        if email_hmac_key.expose().len() < MIN_LOOKUP_KEY_BYTES {
            return Err(PublicIntakeStoreError::InvalidLookupKey);
        }

        let source_host = match source_host {
            PublicIntakeSourceHostV1::User => "user.zpkg.net",
            PublicIntakeSourceHostV1::Organization => "org.zpkg.net",
        };
        if kind == PublicIntakeSubmissionKind::QuoteRequest
            && source_host != "org.zpkg.net"
        {
            return Err(PublicIntakeStoreError::InvalidKindHostPair);
        }

        let mut hmac = <Hmac<Sha256> as Mac>::new_from_slice(email_hmac_key.expose())
            .map_err(|_| PublicIntakeStoreError::InvalidLookupKey)?;
        hmac.update(normalized_email.as_bytes());
        let email_lookup_hmac: [u8; SHA256_BYTES] = hmac.finalize().into_bytes().into();

        let email_aad = format!("{EMAIL_AAD_V1}\n{request_id}");
        let payload_aad = format!(
            "{PAYLOAD_AAD_V1}\n{request_id}\n{}\n{source_host}",
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
            body_sha256,
            email_lookup_hmac,
            normalized_email_ciphertext,
            normalized_payload_ciphertext,
            consented_at,
            marketing_consent,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PublicIntakeInsertResult {
    pub request_id: Uuid,
    pub inserted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PublicIntakeStoreError {
    Database(String),
    EncryptionFailed,
    IdempotencyConflict,
    InvalidBodyDigest,
    InvalidEncryptionKey,
    InvalidKindHostPair,
    InvalidLookupKey,
    InvalidNormalizedEmail,
    InvalidRequestId,
}

impl fmt::Display for PublicIntakeStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Database(_) => "public intake database operation failed",
            Self::EncryptionFailed => "public intake encryption failed",
            Self::IdempotencyConflict => "public intake idempotency conflict",
            Self::InvalidBodyDigest => "public intake body digest is invalid",
            Self::InvalidEncryptionKey => "public intake encryption key is invalid",
            Self::InvalidKindHostPair => "public intake kind and source host are inconsistent",
            Self::InvalidLookupKey => "public intake lookup key is invalid",
            Self::InvalidNormalizedEmail => "public intake normalized email is invalid",
            Self::InvalidRequestId => "public intake request id is invalid",
        })
    }
}

impl std::error::Error for PublicIntakeStoreError {}

impl PublicIntakeStoreError {
    fn from_db(error: impl fmt::Display) -> Self {
        let _ = error;
        Self::Database("redacted".to_owned())
    }
}

/// Insert an encrypted request exactly once.
///
/// A replay of the same request identifier and body is accepted without a
/// second row. Reusing the request identifier with different content is a
/// conflict. A uniqueness collision on the keyed email fingerprint is treated
/// as an enumeration-resistant duplicate and is not surfaced to the caller.
pub async fn insert_public_intake_submission(
    database: &DatabaseConnection,
    input: NewPublicIntakeSubmission,
) -> Result<PublicIntakeInsertResult, PublicIntakeStoreError> {
    let request_id = input.request_id;
    let kind = input.kind.as_str().to_owned();
    let source_host = input.source_host.to_owned();
    let body_sha256 = input.body_sha256.to_vec();

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
                body_sha256,
                email_lookup_hmac,
                normalized_email_ciphertext,
                payload_ciphertext,
                consented_at,
                marketing_consent
            ) values ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            on conflict do nothing
            returning request_id
            "#,
            vec![
                request_id.into(),
                kind.clone().into(),
                source_host.clone().into(),
                body_sha256.clone().into(),
                input.email_lookup_hmac.to_vec().into(),
                input.normalized_email_ciphertext.into(),
                input.normalized_payload_ciphertext.into(),
                input.consented_at.into(),
                input.marketing_consent.into(),
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
            select body_sha256, kind, source_host
            from zed_public_intake_submissions
            where request_id = $1
            "#,
            vec![request_id.into()],
        ))
        .await
        .map_err(PublicIntakeStoreError::from_db)?;

    if let Some(existing) = existing {
        let existing_digest: Vec<u8> = existing
            .try_get("", "body_sha256")
            .map_err(PublicIntakeStoreError::from_db)?;
        let existing_kind: String = existing
            .try_get("", "kind")
            .map_err(PublicIntakeStoreError::from_db)?;
        let existing_source_host: String = existing
            .try_get("", "source_host")
            .map_err(PublicIntakeStoreError::from_db)?;
        if existing_digest != body_sha256
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

#[cfg(test)]
mod tests {
    use super::*;

    fn secret(byte: u8) -> SecretBytes {
        SecretBytes::new(vec![byte; 32])
    }

    #[test]
    fn encrypted_submission_never_formats_contact_or_payload_data() {
        let submission = NewPublicIntakeSubmission::encrypted(
            PublicIntakeSubmissionKind::PreInterest,
            PublicIntakeSourceHostV1::User,
            "018f5f52-feb8-7d4a-a9d6-69d8a1559e8b",
            &"ab".repeat(32),
            "Private.Person@Example.COM",
            br#"{"email":"private.person@example.com","note":"private requirement"}"#,
            Utc::now(),
            false,
            &secret(7),
            &secret(9),
        )
        .expect("valid encrypted submission");

        let debug = format!("{submission:?}");
        assert!(!debug.contains("Private.Person"));
        assert!(!debug.contains("private.person"));
        assert!(!debug.contains("private requirement"));
        assert!(debug.contains("[REDACTED]"));
    }

    #[test]
    fn quote_requests_are_bound_to_the_organization_host() {
        let error = NewPublicIntakeSubmission::encrypted(
            PublicIntakeSubmissionKind::QuoteRequest,
            PublicIntakeSourceHostV1::User,
            "018f5f52-feb8-7d4a-a9d6-69d8a1559e8b",
            &"ab".repeat(32),
            "person@example.com",
            b"{}",
            Utc::now(),
            false,
            &secret(7),
            &secret(9),
        )
        .expect_err("quote requests must use org.zpkg.net");

        assert_eq!(error, PublicIntakeStoreError::InvalidKindHostPair);
    }

    #[test]
    fn database_errors_are redacted() {
        let error = PublicIntakeStoreError::from_db("email=private.person@example.com");
        assert_eq!(error.to_string(), "public intake database operation failed");
        assert!(!format!("{error:?}").contains("private.person"));
    }

    #[test]
    fn schema_marker_matches_transport_authority() {
        assert_eq!(PUBLIC_INTAKE_SCHEMA_V1, "zed.public-intake.v1");
    }
}
