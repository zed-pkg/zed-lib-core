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
- exact shared-definitions provenance rather than independently authored product DDL.

## Reconciled decisions

### One package, not two ORM authorities

The canonical Zed package is:

```text
zed-pkg/zed-lib-core@0.1.0
```

Its Rust ORM target remains the crate/package identity `zed-orm-core` for consumer compatibility, but that crate now lives only inside this repository at `src/rust-orm`. The former standalone repositories are historical sources, not parallel release authorities.

### Canonical database ownership

The declarative registry schema is owned by:

```text
ORESoftware/k8s-libs-and-shared-defs
pg-defs/schema/orgs/zed-pkg/registry.sql
```

The exact merged revision and blob are recorded in `shared-defs.lock.json`. The vendored SQL in `src/rust-orm/sql/registry.sql` must remain byte-identical. This crate may apply that reviewed segment through an advisory-locked version ledger; it must not invent divergent DDL.

### Existing VCS registry preserved

`pg-defs/schema/orgs/zed-pkg/vcs.sql` continues to own the separate `vcs_*` mirror/operations tables used by the VCS service. The package-registry `zed_*` tables do not replace or absorb those tables.

### Auth versus authorization

Supabase and shared-auth own identity ceremonies, sessions, MFA, and device policy. The registry owns organization/project/package authorization through local memberships and invitations. Cross-database shared-auth subjects are application-validated references, not same-database foreign keys.

### API and registry hierarchy

The broad public API lives under `/api/v1`. Registry operations are the special subset `/api/v1/registry`, not a second top-level service. The complete route, page, compatibility, and R2 contract is machine-readable in `contracts/api-routes.v1.json`.

### R2 versus Postgres authority

Cloudflare R2 stores immutable artifact bytes under content-addressed keys. Postgres remains authoritative for package identity, visibility, versions, digest/size/format, upload state, download facts, and authorization. The API commits the download ledger before returning a signed R2 redirect. Neither the API nor app.zpkg.net proxies artifact bytes.

## Compatibility and release gates

- No synthetic `.zpkg.lock` may be committed. A release lock must come from a successful immutable Zed resolution.
- `zed-api-server.rs` consumes the write-enabled ORM target; `zed-web-server.rs` consumes the default read-only target.
- API and web consumers must pin the same reviewed `zed-lib-core` revision and route-contract version.
- Shared-schema changes land in shared definitions first, regenerate adapters, and are then repinned here.
- Database migration, API rollout, web rollout, and R2 activation remain separate reviewed deployment gates.
