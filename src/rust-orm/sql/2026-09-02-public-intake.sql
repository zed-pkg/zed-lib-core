-- Append-only encrypted persistence for public commercial intake.
--
-- Plaintext email addresses and normalized JSON payloads never enter this table.
-- The API supplies request identifiers, keyed lookup fingerprints, body digests,
-- and AES-256-GCM envelopes after validating the signed Cloudflare ingress.

create table if not exists zed_public_intake_submissions (
    request_id uuid primary key,
    kind text not null,
    source_host text not null,
    body_sha256 bytea not null,
    email_lookup_hmac bytea not null,
    normalized_email_ciphertext bytea not null,
    payload_ciphertext bytea not null,
    consented_at timestamptz not null,
    marketing_consent boolean not null,
    submitted_at timestamptz not null default statement_timestamp(),
    constraint zed_public_intake_kind_check
        check (kind in ('pre_interest', 'quote_request')),
    constraint zed_public_intake_source_host_check
        check (source_host in ('user.zpkg.net', 'org.zpkg.net')),
    constraint zed_public_intake_kind_host_check
        check (kind <> 'quote_request' or source_host = 'org.zpkg.net'),
    constraint zed_public_intake_body_digest_check
        check (octet_length(body_sha256) = 32),
    constraint zed_public_intake_email_hmac_check
        check (octet_length(email_lookup_hmac) = 32),
    constraint zed_public_intake_email_ciphertext_check
        check (octet_length(normalized_email_ciphertext) >= 28),
    constraint zed_public_intake_payload_ciphertext_check
        check (octet_length(payload_ciphertext) >= 28),
    constraint zed_public_intake_email_kind_unique
        unique (email_lookup_hmac, kind)
);

create index if not exists zed_public_intake_submitted_at_idx
    on zed_public_intake_submissions (submitted_at);

revoke all on table zed_public_intake_submissions from public;
