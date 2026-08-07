# @zed-pkg/zed-lib

Implementations of the [zed-pkg](https://github.com/zed-pkg) contract in
TypeScript, so a browser client answers "which version does `^1.2` install?"
exactly the way `zed-cli` does.

```ts
import type { PackageMetadata } from "@zed-pkg/zed-interfaces";
import { ResolveError, resolveVersion } from "@zed-pkg/zed-lib";

try {
  const version = resolveVersion(metadata, "^1.2"); // "1.4.0", as published
} catch (error) {
  if (error instanceof ResolveError) {
    error.kind; // "no_versions" | "invalid_requirement" | "unsatisfied"
  }
}
```

Types come from `@zed-pkg/zed-interfaces`; this package adds only behavior.

## Not a binding, and not a dependency wrapper

This is a native TypeScript implementation of the same algebra, held to
[`conformance/cases/*.json`](../../conformance) alongside the Rust and Dart
slices. It is deliberately dependency-free: npm's `semver` implements a
different dialect from Cargo's — a bare `1.0.0` is exact there, a caret range
here, and `1.2` means the 1.2 line rather than `^1.2` — and three
implementations of one contract cannot afford a translation layer whose edge
cases nobody reads.

Shipped as TypeScript source. Everything is erasable syntax, so Node runs the
tests with no build step:

```sh
npm install
npm test           # node --test conformance.test.ts, the shared corpus
npm run typecheck
```
