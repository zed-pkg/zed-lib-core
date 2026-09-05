# zed-lib-core semantic merge provenance

`zed-lib-core` is the canonical merged core library for the `zed-pkg` organization. It preserves the useful behavior and history of the former `zed-lib` repository and the owned core implementation historically named `zed-orm-core`.

The user-facing term **zed-core** is interpreted here as `zed-pkg/zed-orm-core`; no separate owned `zed-pkg/zed-core` repository existed during the merge audit.

## History union

The two-history merge commit is:

```text
f27f72cc65640407409d38953c8d30ee4c95f3a6
```

Its parents are:

```text
430aafe24b6c3ab1263f1351ab4941545f592f19  # zed-lib lineage
 a5dabf3685db94ffdf5ae30cb3b3e4cc1cce298f  # zed-orm-core lineage
```

The semantic fold is:

```text
9fdc5fed96b707b99b3b02e6541060831c3d70fd
```

The merge was not a choose-one-side conflict resolution. The temporary verbatim `src/rust-orm-core` import was folded into the existing `src/rust-orm` package and the overlapping responsibilities were reconciled explicitly.

## Retained from zed-lib

- canonical version and requirement parsing behavior;
- latest-stable and version-resolution logic;
- Rust, Dart, and TypeScript public APIs;
- the shared cross-language conformance corpus;
- whole-repository Zed package targets;
- registry entities and named account/package operations already implemented in the library lineage.

## Retained from zed-orm-core

- opaque `ReadContext` and feature-gated `WriteContext` rather than public raw SeaORM connections;
- database-role verification and `default_transaction_read_only` enforcement;
- bounded pool policy and statement timeouts;
- compile-fail/default-surface proof that write symbols are absent without the write feature;
- the requirement that a SELECT-only database role is the authoritative web-tier security boundary;
- exact historical shared-definitions import provenance without retaining it as a runtime or package dependency.

## Reconciled decisions

### One package, not two ORM authorities

The canonical Zed package is:

```text
zed-pkg/zed-lib-core@0.1.0
```

Its Rust ORM package remains the crate/package identity `zed-orm-core` for consumer compatibility, but that crate now lives only inside this repository at `src/rust-orm` and publishes from its nested manifest. The former standalone repositories are historical sources, not parallel release authorities.

### Canonical database ownership

The declarative registry schema is now owned by:

```text
zed-pkg/zed-lib-core
src/rust-orm/sql/registry.sql
```

The shared-definitions revision and blob recorded in `shared-defs.lock.json` are immutable historical import evidence. They continue to explain the original merge and deployed ledger identities, but new DDL is reviewed here. The dependency-free nested manifest under `src/rust-orm/sql` publishes as `zed-pkg/zed-schema`; that is the only package consumed by declarative-migrations. The sibling nested manifest under `src/rust-orm` publishes as `zed-pkg/zed-orm-core` for product servers.

### Existing VCS registry preserved

`pg-defs/schema/orgs/zed-pkg/vcs.sql` continues to own the separate `vcs_*` mirror/operations tables used by the VCS service. The package-registry `zed_*` tables do not replace or absorb those tables.

### Auth versus authorization

Supabase and shared-auth own identity ceremonies, sessions, MFA, and device policy. The registry owns organization/project/package authorization through local memberships and invitations. Cross-database shared-auth subjects are application-validated references, not same-database foreign keys.

### API and registry hierarchy

The broad public API lives under `/api/v1`. Registry operations are the special subset `/api/v1/registry`, not a second top-level service. The complete route, page, compatibility, and R2 contract is machine-readable in `contracts/api-routes.v1.json`.

### R2 versus Postgres authority

Cloudflare R2 stores immutable artifact bytes under content-addressed keys. Postgres remains authoritative for package identity, visibility, versions, digest/size/format, upload state, download facts, and authorization. The API commits the download ledger before returning a signed R2 redirect. Neither the API nor app.zpkg.net proxies artifact bytes.

## Third lineage: zed-lock (2026-09-04)

`zed-pkg/zed-lock` — the kernel-backed, event-driven local file-lock crate
extracted from `zed-cli` — was folded in as the `src/rust-lock` slice. Its
history is a parent of the fold merge commit recorded in
`PREDECESSOR_MIGRATION.md` ("zed-lock lineage"); the lineage's tip on the
standalone repository was `7818d0140f9947352f803d4a50aabb8e0b26265a`, followed
by one mechanical relocation commit that moved every tracked file under
`src/rust-lock` so the merge itself carries no content changes.

### Retained from zed-lock

- the crate name `zed-lock` and its entire public API (`LockManager`,
  `LockRequest`, `LockClass`, `PathSecurityPolicy`, guards and waiters);
- the descriptor lock as the sole local ownership authority — no polling, no
  PID files, no network in the local path;
- the Quint waiter-lifecycle model under `src/rust-lock/formal` and the
  protocol corpus it is checked against;
- the package-contract checker and its negative-case tests, re-pointed at the
  nested-slice invariants;
- the standalone workflows, kept under `src/rust-lock/.github-zed-lock` as
  reference only (the repository's `ci.yml` runs the crate).

### Reconciled decisions

**One crate, one release authority.** The crate keeps its name so consumers
change only where they fetch it from. It is published as the nested
`zed-pkg/zed-lock` package through the root manifest's `targets.rust-lock`
under the `lock/v{version}` tag namespace; the nested `.zpkg.toml` declares
no targets of its own. `cargo publish` to crates.io stays an independent
operation, as before.

**Local locking stays local.** `zed-lock` still has no dependency on the ORM
slice, on `sea-orm`, or on the network. Fiducia leases and Postgres advisory
locks are composed *around* it, not inside it — that composition is the job of
[`ORESoftware/ores-locks-and-leases`](https://github.com/ORESoftware/ores-locks-and-leases),
which zed-lib-core will import rather than reimplement.

**The standalone repository is retired, not deleted.** `zed-pkg/zed-lock` is
archived with a pointer here; its tags remain valid for the versions they
named.

## Compatibility and release gates

- No synthetic `.zpkg.lock` may be committed. A release lock must come from a successful immutable Zed resolution.
- `zed-api-server.rs` consumes the write-enabled ORM package; `zed-web-server.rs` consumes the same package with its default read-only features.
- API and web consumers must pin the same reviewed `zed-lib-core` revision and route-contract version.
- Zed schema changes land in this repository first, regenerate SeaORM and Drizzle shadow artifacts, and are released as one immutable Zed package before any consumer pin moves.
- Database migration, API rollout, web rollout, and R2 activation remain separate reviewed deployment gates.
