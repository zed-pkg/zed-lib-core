# Production peer-authority runtime conformance

Tracks DEN-3828 / DEN-3600 and ORESoftware/typespec-json-schema-validator#20.
This suite imports the merged production gate from zed-pkg/zed-interfaces#90;
it does not copy the TypeSpec/JSON Schema declarations into this repository.

The mandatory workflow first verifies immutable dependency checkouts, compiles
both independent public authorities, checks all four declaration identities and
the exact-input Contract IR, and executes their recorded valid/invalid corpus.
Only then does it compare actual runtime behavior. The TypeScript test consumes
the frozen admitted corpus directly. The Rust runner reads the same pinned corpus
within that job after the production gate succeeds. Missing compiler/checkout or
any recorded disagreement fails; no runtime gate is skipped as a substitute.

## Corrected wire behavior

- `PageQuery.limit` is required. A schema default of 50 is an annotation; callers
  must explicitly choose it before validation. Missing fields are not inserted.
- Validation does not trim strings or change accepted values. UI/application
  normalization may be explicit before the wire boundary, not hidden within it.
- Optional `locale`, `cursor`, and `detail` may be absent but may not be null.
  Rust deserialization distinguishes those states and omits absent fields when
  serializing, instead of emitting schema-invalid nulls.
- String limits count Unicode scalar values. Rust garde uses `length(chars)`;
  the tested Zod 4.5.4 already implements the required semantics and is pinned.
- Rust accepts mathematical JSON integers such as 50.0 or 5e1 but never strings,
  fractions, null, booleans, nonfinite numbers or values outside u16. Field range
  checks still apply. The numeric helper targets self-describing JSON decoding,
  not a new promise of compatibility with non-self-describing binary formats.

The `ores.validation.v1` data contract is unchanged. These are implementation
corrections with observable behavior changes: callers relying on implicit trim,
default insertion, or null optional strings must supply contract-valid values.
No business default, authorization decision, or server-private policy is added.

## Commands

Provision the checkouts in `pins.json` using the workflow's nested `.deps` layout.
The public TypeScript lock records the exact versions and registry integrities
observed in the test-first baseline. Rust uses the existing root workspace lock;
no new Rust dependency or independent nested lock was introduced.

```sh
npm ci --prefix .deps/zed-interfaces/.deps/typespec-json-schema-validator
node .deps/zed-interfaces/validation/compiler/public-parity.mjs
npm ci --prefix validation/typescript
npm test --prefix validation/typescript
node --test validation/compiler-conformance/runtime.test.mjs
cargo test --locked -p zed-validation -p zed-validation-server
cargo run --locked -p zed-validation-server --example production_corpus
```

The explicit Rust example is a mandatory conformance runner, not a demonstration
of mock behavior: it decodes/validates/serializes the actual library types and
fails on disagreements. It lives in the existing serde_json-owning crate, keeping
public library dependencies and the workspace lock unchanged. Ordinary native
wire regressions are also always run by that crate's standard `cargo test`.

The aggregate contract is checked as the exclusive union of the three actual
runtime validators. This does not introduce a fourth independent implementation.
This bounded corpus is not proof of universal schema equivalence. Private schema
compilation, Go/Gleam/Dart production corpus adoption, application caller rollout,
and release-admission enforcement remain separate work.
