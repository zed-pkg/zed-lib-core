# Public commercial-intake persistence

The Rust API authenticates and validates `zed.public-intake.v1`, then calls the named `zed_orm_core::public_intake::write_public_intake_submission` operation through an opaque `WriteContext`.

The storage contract deliberately exposes only route identity, fixed-size request and email fingerprints, consent flags, timestamps, and replay counts. PostgreSQL `pgcrypto` encrypts both the normalized email and the complete normalized JSON payload with AES-256-class OpenPGP symmetric encryption. No plaintext email, name, organization, website, role, or requirements summary column exists.

A request ID is the idempotency authority. A byte-identical replay atomically updates `last_seen_at` and `replay_count`; reuse with a different body digest, route kind, or source host returns no row and is rejected. There is no read-before-write race.

The encryption key is supplied only as a bound value by the API process. It does not belong in SQL, migrations, logs, metrics, URLs, source control, or database metadata. Only the API and discrete migrator identities may receive table privileges; the migration revokes `PUBLIC` access.

TypeSpec, Protobuf, JSON Schema, Rust DTOs, Diesel models, and SeaORM operations remain separate checked representations. This module is the SeaORM write boundary; it does not make SeaORM entities public DTOs and does not replace the other schema technologies.
