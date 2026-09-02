# Public commercial-intake persistence

This repository owns the durable database boundary for `zed.public-intake.v1` after the Cloudflare edge and API have validated the request. It does not own the public form, Turnstile verification, transport signature, or API routing.

## Stored representation

`zed_public_intake_submissions` stores:

- the caller-generated UUID used for idempotency;
- the bounded intake kind and exact source host;
- a 32-byte digest of the normalized canonical request;
- a 32-byte HMAC of the normalized email for duplicate suppression;
- independent AES-256-GCM envelopes for the normalized email and full normalized payload;
- consent time, marketing-consent state, and server submission time.

No plaintext email, name, organization, website, referral value, requirements summary, IP address, Turnstile token, API credential, or signing key belongs in the table or in application diagnostics.

## Replay behavior

A replay with the same request UUID, source host, kind, and body digest succeeds without a second row. Reusing a UUID for different content fails as an idempotency conflict. A duplicate keyed email fingerprint for the same intake kind is treated as an accepted duplicate so the public response cannot reveal whether the address was already known.

## Key boundaries

The API supplies two independent secret values at runtime:

- a 32-byte AES key for payload encryption;
- a separate HMAC key of at least 32 bytes for lookup fingerprints.

Neither key is stored in PostgreSQL or committed to Git. The application must rotate keys through an explicitly versioned migration and re-encryption plan; silently replacing either key would make existing rows unreadable or undiscoverable.

## Schema and ORM relationship

The append-only migration is also included in the declarative registry SQL, so the existing schema generator and SeaORM projections observe the final table. The public API continues to use the opaque `WriteContext`/named-operation boundary rather than exposing raw ORM entities. TypeSpec, Protobuf, JSON Schema, Rust, Dart, and TypeScript transport representations remain owned by `zed-interfaces`.
