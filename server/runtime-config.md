# Runtime configuration boundary

`zed-lib` owns a pure configuration materializer rather than a process-environment reader.

1. Each executable runs the pinned `flags-2-env` entrypoint exactly once.
2. The executable snapshots the resulting environment into an immutable `BTreeMap<String, String>`.
3. With the Rust `server-config` feature enabled, it declares every accepted source key as public, server-only, or secret and calls `ServerRuntimeConfig::materialize`.
4. Only `public_projection()` may cross into browser, Flutter, edge, generated-client, or serialized bootstrap configuration.

The default crate feature exposes only `PublicRuntimeConfig`. Server values and `SecretValue` do not exist in default/client builds. The library never reads `std::env`, never parses command-line flags, and never decrypts secrets; those effects stay at executable startup boundaries.

Public output keys are lowercase and reject secret-shaped names such as `token`, `password`, `credential`, `database_url`, and `private_key`. Secret debug output is redacted. Duplicate declarations, missing required values, empty values, and invalid key syntax fail closed.
