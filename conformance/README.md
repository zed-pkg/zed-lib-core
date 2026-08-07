# Conformance corpus

Language-neutral cases that **every** zed-lib implementation must satisfy. The
Rust slice runs them from `src/rust/tests/conformance.rs`; the Dart and
TypeScript slices will run the same files.

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

## Adding a case

Add it here first and watch it fail, then implement. A case that was written
after the code tests the code's opinion rather than the contract.
