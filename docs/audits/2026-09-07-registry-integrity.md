# Registry integrity follow-up — DEN-3908

## Scope

The inherited registry DDL has independent package and version foreign keys.
They prove that each identifier exists, but not that the version belongs to the
claimed package. Its optional project pointer likewise does not bind the project
to the package's organization. The SPDX check can evaluate to SQL NULL when the
identifier is missing, which PostgreSQL accepts as a satisfied CHECK.

The forward-only `2026-09-07-registry-integrity.sql` adds four composite foreign
keys and an explicit non-null guard for SPDX rows. Two unique indexes provide
composite reference keys. No columns, public wire types, authentication grants,
or generated ORM files are changed. Existing single-column foreign keys remain.
This SQL boundary applies equally to SeaORM, Diesel and direct SQL writers; it
is not a claim of completed two-ORM runtime parity or full tenant authorization.

## Migration safety

Run through the serialized, transactional one-shot migrator, never on web/API
startup. Historical SQL and migration identities remain unchanged. Every new
constraint is validated before success. A dirty historical row aborts the
transaction; no data is erased, reassigned or silently repaired. Reapplying the
migration is supported, but changing its bytes after deployment is not.

The unique-index builds and validation need locks and scans. Review table sizes,
backups, statement/lock timeouts and the maintenance window before production
adoption. For large tables, design a separately reviewed concurrent-index rollout;
do not run CREATE INDEX CONCURRENTLY inside this migration's transaction. This
change does not deploy to Supabase, Neon or RDS or change a disabled endpoint.

Nullable project/version associations stay legal. Deleting a project clears
only project_id, never org_id. Deleting a version cascades version-specific
licenses and clears only the download/upload version pointer. The older verified
upload check still pins versions referenced by verified uploads; this patch does
not weaken that invariant. Package deletion retains the existing cascade policy.
Lifetime download counters must not be reduced by rejected writes or pruning.

## Verification

The PostgreSQL 17/18 workflow runs the same twelve adversarial writes against
both the historical and upgraded schema. Historical acceptance is required to
prove the bugs were reproduced; the upgraded run requires the expected SQLSTATE
and constraint identity. It also checks parent reassignments, counter rollback,
nullable relationships, delete actions, repeat application and atomic rejection
of two kinds of dirty historical database. Prior publication-policy regressions
run after the new migration as well. Consult actual CI for execution results.

## Remaining audit boundaries

Private contract/compiler and table-coverage work remains in zed-interfaces #92.
This does not add zed_clis, certify generated Diesel/SeaORM parity, establish RLS
or exercise live customer accounts. Dependency-graph edge bindings, polymorphic
embedding ownership and full download-ledger immutability require separate review.

Primary PostgreSQL references:
- https://www.postgresql.org/docs/18/ddl-constraints.html
- https://www.postgresql.org/docs/18/sql-altertable.html
