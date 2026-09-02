-- Forward-only public commercial-intake persistence.
--
-- The application binds every value and uses pgcrypto for contact data and the
-- complete normalized payload. Only fixed-size fingerprints, consent flags,
-- route identity, and timestamps remain queryable without decryption.

create extension if not exists pgcrypto;

create table if not exists zed_public_intake_submissions (
    request_id uuid primary key,
    kind text not null,
    source_host text not null,
    body_sha256 bytea not null,
    email_lookup_hmac bytea not null,
    encrypted_email bytea not null,
    encrypted_payload bytea not null,
    consented_at timestamptz not null,
    contact_consent boolean not null,
    marketing_consent boolean not null,
    first_seen_at timestamptz not null default now(),
    last_seen_at timestamptz not null default now(),
    replay_count bigint not null default 0,

    constraint zed_public_intake_kind_chk
        check (kind in ('pre_interest', 'quote_request')),
    constraint zed_public_intake_source_host_chk
        check (source_host in ('user.zpkg.net', 'org.zpkg.net')),
    constraint zed_public_intake_kind_host_chk
        check (
            (kind = 'pre_interest' and source_host = 'user.zpkg.net')
            or (kind = 'quote_request' and source_host = 'org.zpkg.net')
        ),
    constraint zed_public_intake_body_sha256_chk
        check (octet_length(body_sha256) = 32),
    constraint zed_public_intake_email_lookup_hmac_chk
        check (octet_length(email_lookup_hmac) = 32),
    constraint zed_public_intake_encrypted_email_chk
        check (octet_length(encrypted_email) >= 32),
    constraint zed_public_intake_encrypted_payload_chk
        check (octet_length(encrypted_payload) >= 32),
    constraint zed_public_intake_contact_consent_chk
        check (contact_consent),
    constraint zed_public_intake_replay_count_chk
        check (replay_count >= 0),
    constraint zed_public_intake_seen_order_chk
        check (last_seen_at >= first_seen_at)
);

create index if not exists zed_public_intake_kind_email_idx
    on zed_public_intake_submissions (kind, email_lookup_hmac);

create index if not exists zed_public_intake_first_seen_idx
    on zed_public_intake_submissions (first_seen_at desc);

revoke all on table zed_public_intake_submissions from public;
