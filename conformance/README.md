# Conformance corpus

Language-neutral cases that **every** zed-lib implementation must satisfy. The
Rust slice runs them from `src/rust/tests/conformance.rs`; the Dart and
TypeScript slices run the same files in their conformance tests.

The hand-written, model-derived, and generated fuzz corpora are validated in CI against the
JSON Schema 2020-12 contracts in `conformance/schema/`. A case that is
syntactically plausible but outside the shared wire contract must fail schema
validation before any implementation is allowed to interpret it.

A corpus, rather than per-language tests, is what keeps three implementations
honest: "the CLI resolved 1.4.0 but the web UI offered 2.0.0" becomes a failing
test in one of them instead of a support ticket.

## `cases/version-resolution.json`

Each case is `(scheme, published versions, requirement)` and expects either a
resolved version or an error kind:

```json
{
  "name": "an opaque package refuses range algebra",
  "scheme": "opaque",
  "versions": ["legacy-api"],
  "requirement": "^1.0",
  "expect": { "error": "invalid_requirement" }
}
```

`expect` carries exactly one of `version` (the published spelling, never a
normalized one) or `error` (`no_versions`, `invalid_requirement`,
`unsatisfied` — the strings `ResolveError::kind()` returns).

Two cases exist because the answer surprises people, and a corpus is the right
place to pin a surprise: a bare `1.0.0` requirement is a **caret range** in this
stack, so it resolves to `1.0.1` when that is published. `=1.0.0` is how a
requirement pins one version.

## `cases/latest-stable.json`

Cases for `latest_stable`, whose `expect` carries only `version` (it returns
null, never an error). `latest` is **data** in these cases, including when it is
null — that is what "the registry recorded nothing" looks like, and a runner
that substitutes the newest version there is quietly answering a different
question. Resolution cases never read `latest`, so they may fall back.

## `cases/formal-*.json` — model-derived, do not edit

These 34 cases are projected from completed Quint states: 17 input fixtures,
each executed twice through the actual model transitions. The Rust projector
does **not** call any resolver to compute expected values. All three production
implementations independently replay those results, including error kinds and
exact published spelling. Removing either corpus file fails the loaders.

From the repository root, with the pinned toolchain available:

```sh
mkdir -p .formal-artifacts/dependency-resolution
npx --yes @informalsystems/quint@0.32.0 run formal/dependency_resolution.qnt \
  --backend typescript --step trace_step --invariant resolution_safety \
  --seed 3855 --max-samples 1 --max-steps 180 \
  --out-itf .formal-artifacts/dependency-resolution/trace.itf.json --verbosity 0
cargo test --locked -p zed-lib --example generate_formal_corpus
cargo run --locked -p zed-lib --example generate_formal_corpus
```

Generation rejects incomplete, failed, conflicting, or nondeterministic traces.
ITF timestamps and intermediate states are not included in committed output.
CI repeats generation and requires no diff. The separate exhaustive TLC job
checks the full finite state space, not just this deterministic replay path.
See [the formal boundary](../formal/README.md#dependency-resolution-boundary)
for parser assumptions and the distinction between scalar and graph resolution.

## `cases/fuzz-*.json` — generated, do not edit

```sh
cargo run --locked --example generate_fuzz_corpus
```

600 pseudo-random cases answered by the **Rust** implementation and replayed
against Dart and TypeScript. Rust is the oracle because it delegates the hard
part to the same `semver` crate Cargo uses, so the property under test is "the
hand-written Dart and TypeScript algebra agrees with Cargo". Deterministic
(fixed-seed LCG, no clock, no entropy), so regenerating rewrites the files byte
for byte and CI can diff them.

It earned its keep on the first run: 16 cases diverged, in three classes, in
both hand-written implementations independently.

* An operator may be separated from its version — `>= 1.0.0, < 2.0.0`. Both
  tokenizers split on whitespace and orphaned the operator, turning a valid
  range into an opaque tag.
* A requirement with leading zeros (`2026.07.24`) is **not** a range: semver
  forbids leading zeros in numeric identifiers, so Cargo parses it as an exact
  tag. Both had accepted it as a caret range, which also produced a spurious
  `invalid_requirement` for opaque packages.
* Ties go to the **last** equal element. Distinct spellings can parse to the
  same version (`1.0.0` and `v1.0.0`, `1.2.3` and `1.2.3.post1`); Rust resolves
  with `Iterator::max_by`, which returns the last maximum, and both others kept
  the first. The tie-break decides which spelling gets installed.

Each is now also a named case in `version-resolution.json`, because a generated
file records the answer but not the intent.

## Adding a case

Add it here first and watch it fail, then implement. A case that was written
after the code tests the code's opinion rather than the contract.

Verify a new hand-written case against Rust before committing it: Rust is what
consumers already run through `zed-cli`, so a case Rust fails is a case that
mis-states the contract.
