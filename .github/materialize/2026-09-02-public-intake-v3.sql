-- Append-only encrypted persistence for public commercial intake.
--
-- Plaintext contact fields, normalized JSON payloads, IP addresses, abuse proofs,
-- and raw body digests never enter this table. The API supplies versioned,
-- domain-separated keyed fingerprints and AES-256-GCM envelopes.

create table if not exists zed_public_intake_submissions (
    request_id uuid primary key,
    kind text not null,
    source_host text not null,
    body_fingerprint_hmac bytea not null,
    encryption_format text not null,
    fingerprint_format text not null,
    encryption_key_id text not null,
    email_hmac_key_id text not null,
    email_lookup_hmac bytea not null,
    normalized_email_ciphertext bytea not null,
    payload_ciphertext bytea not null,
    consented_at timestamptz not null,
    marketing_consent boolean not null,
    retention_expires_at timestamptz not null,
    submitted_at timestamptz not null default statement_timestamp(),
    constraint zed_public_intake_kind_check
        check (kind in ('pre_interest', 'quote_request')),
    constraint zed_public_intake_source_host_check
        check (source_host in ('user.zpkg.net', 'org.zpkg.net')),
    constraint zed_public_intake_kind_host_check
        check (kind <> 'quote_request' or source_host = 'org.zpkg.net'),
    constraint zed_public_intake_body_fingerprint_check
        check (octet_length(body_fingerprint_hmac) = 32),
    constraint zed_public_intake_encryption_format_check
        check (encryption_format = 'aes-256-gcm-v1'),
    constraint zed_public_intake_fingerprint_format_check
        check (fingerprint_format = 'hmac-sha256-v1'),
    constraint zed_public_intake_encryption_key_id_check
        check (encryption_key_id ~ '^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$'),
    constraint zed_public_intake_email_hmac_key_id_check
        check (email_hmac_key_id ~ '^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$'),
    constraint zed_public_intake_email_hmac_check
        check (octet_length(email_lookup_hmac) = 32),
    constraint zed_public_intake_email_ciphertext_check
        check (octet_length(normalized_email_ciphertext) between 28 and 512),
    constraint zed_public_intake_payload_ciphertext_check
        check (octet_length(payload_ciphertext) between 28 and 20000),
    constraint zed_public_intake_retention_check
        check (
            retention_expires_at >= consented_at + interval '1 day'
            and retention_expires_at <= consented_at + interval '3650 days'
        ),
    constraint zed_public_intake_email_kind_unique
        unique (email_hmac_key_id, email_lookup_hmac, kind)
);

create index if not exists zed_public_intake_submitted_at_idx
    on zed_public_intake_submissions (submitted_at);

create index if not exists zed_public_intake_retention_expiry_idx
    on zed_public_intake_submissions (retention_expires_at, request_id);

revoke all on table zed_public_intake_submissions from public;
