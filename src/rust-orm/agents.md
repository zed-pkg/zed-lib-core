# Rust persistence instructions

These instructions refine the repository-root `agents.md` for `src/rust-orm`.

- Apply the organization-wide functional/effect-boundary guidance from `https://github.com/ORESoftware/my-ai/blob/main/AGENTS.md` together with every readable ancestor `agents.md`.
- Keep transport DTOs out of ORM entities. Validate through `zed-interfaces`, convert to opaque domain inputs, and expose named read/write operations rather than raw connections or query builders.
- Preserve the TypeSpec, Protobuf, JSON Schema, Diesel, and SeaORM architecture as independently checked layers. Do not claim that one representation replaces the others.
- Keep migrations append-only after publication. Changes to desired-state SQL must regenerate and verify all schema/ORM projections.
- Never log or format plaintext contact data, payloads, ciphertext, keyed fingerprints, credentials, database URLs, or encryption/signing keys.
- Public intake writes must remain idempotent, enumeration-resistant, encrypted before persistence, and fail closed on malformed keys, digests, or host/kind combinations.
