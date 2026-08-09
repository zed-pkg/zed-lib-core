# Canonical account-control and machine-publication boundary

`zed-orm-core` is the only application data-plane owner for the Zed account console and registry projection. API servers receive a verified Shared Auth subject, resolve its `zed_users` identity, and call named operations rather than constructing SeaORM queries outside this crate.

## Transactional authorization

Account writes re-read organization and project membership in the same PostgreSQL transaction as the mutation. This prevents membership revocation from racing a previously completed authorization lookup. Project-specific roles and organization roles are combined by explicit precedence (`owner`, `admin`, `member`, `reader`), and cross-organization project assignment is rejected.

The general package-settings operation intentionally has no visibility field. Private-to-public promotion uses its dedicated database-guarded operation so the inclusive age and download limits cannot be bypassed through a generic patch.

## Machine registry adoption

During the `/v1` compatibility cutover, a successful immutable machine publication is adopted into the canonical `zed_*` projection before the API reports success. Adoption is serialized by the public package coordinate and records the organization, package, immutable version, verified R2 upload, and audit fact in one transaction.

Replaying the same version and artifact facts is idempotent. Replaying the same coordinate with a different hash, size, format, storage key, VCS provenance, or manifest is rejected. A package already created privately retains its visibility; only a first package originating in the public machine registry begins public.

The legacy and canonical transactions cannot be globally atomic while two table families remain. The API therefore performs the legacy write first and the idempotent canonical adoption second, returning no success until adoption commits. A retry reconciles a legacy-only partial commit. The final migration removes this compatibility bridge and writes only the canonical tables.

## Required release evidence

A release candidate must pass the default/read-only surface, all-feature read-write surface, migration build, Clippy with warnings denied, unit and conformance tests, generated corpus stability, semantic merge certification, and a full API/web/CLI integration test against one PostgreSQL instance.
