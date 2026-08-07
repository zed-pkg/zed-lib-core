# zed-lib

Implementations of the [zed-pkg](https://github.com/zed-pkg) contract defined
in [zed-interfaces](https://github.com/zed-pkg/zed-interfaces).

```
zed-interfaces   shape     types, serialization, validation
      ▲
      │ depends on
      │
zed-lib          behavior  resolution, planning, policy
      ▲
      │ depends on
      │
zed-cli, zed-api-server, zed-web-server, the front ends
```

`zed-interfaces` answers *is this document well-formed?* — it is on the compile
path of every service and client, so it stays cheap and free of opinion.
`zed-lib` answers *what does it mean?* — the logic that composes those types
into decisions.

## What is here today

`resolve` — scheme-aware version resolution against registry metadata.
`zed_interfaces::version::resolve` takes a bare list of version strings and so
cannot know that a package declared itself `opaque`; resolving one of those
through semver range algebra installs something its publisher never promised.
This takes the whole `PackageMetadata`, lets the package's own `VersionScheme`
decide how the requirement is read, and distinguishes the three ways resolution
fails instead of collapsing them into `None`:

```rust
use zed_lib::{ResolveError, latest_stable, resolve_version};

let version = resolve_version(&metadata, "^1.2")?;   // "1.4.0", as published
let newest = latest_stable(&metadata);               // ignores prereleases
```

| failure               | means                                              |
| --------------------- | -------------------------------------------------- |
| `no_versions`         | the package exists but has nothing installable      |
| `invalid_requirement` | the requirement cannot mean anything here (`^1.x.y`, or a range against an opaque package) |
| `unsatisfied`         | a good requirement nothing published satisfies      |

## Three implementations, one corpus

```
zed-lib/
  src/rust/            the crate (Cargo.toml lives here)
  src/dart/            native Dart implementation (package:zed_lib)
  src/ts/              native TypeScript implementation (@zed-pkg/zed-lib)
  conformance/cases/   language-neutral corpus all three must pass
  Cargo.toml           virtual workspace, members = ["src/rust"]
  .zpkg.toml           one package, one slice per language + the corpus
```

The Dart and TypeScript slices are **not bindings**. Each is a native
implementation of the same algebra, and each runs
[`conformance/cases/*.json`](conformance) — so "the CLI resolved 1.4.0 but the
web UI offered 2.0.0" is a failing test in one of them rather than a support
ticket.

All three are dependency-free on purpose. `pub_semver` and npm's `semver` each
implement a *different dialect* from Cargo's, and they disagree exactly where
it hurts:

| requirement | Cargo (and zed) | npm `semver` | `pub_semver` |
| ----------- | --------------- | ------------ | ------------ |
| `1.0.0`     | `>=1.0.0 <2.0.0` | exactly `1.0.0` | exactly `1.0.0` |
| `1.2`       | `>=1.2.0 <2.0.0` | `>=1.2.0 <1.3.0` | — |
| `1.2.*`     | `>=1.2.0 <1.3.0` | same | — |

Three implementations of one contract cannot afford a translation layer whose
edge cases nobody reads, so the algebra is written out in each language and the
corpus proves they agree.

## Migrating behavior out of zed-interfaces

Behavior that lives in `zed-interfaces` today — `version` parsing, `excludes`
matching, `language` detection — moves here one module at a time. Each move is
a breaking change for the interface crate, so it is tracked as its own ticket
and lands with its consumers updated. **Do not copy a module here while it still
exists there**; depend on it until it moves, so the two can never disagree.

## Development

Sibling checkouts, like the rest of the org:

```sh
git clone https://github.com/zed-pkg/zed-interfaces
git clone https://github.com/zed-pkg/zed-lib
cd zed-lib

cargo test                                        # Rust slice + corpus
(cd src/dart && dart pub get && dart test)        # Dart slice + corpus
(cd src/ts   && npm install && npm test)          # TypeScript slice + corpus
```

Every slice depends on its `zed-interfaces` counterpart by path until
`zed-interfaces` publishes `0.1.0`, at which point each becomes a plain version
requirement.

## License

MIT
