# zed_lib (Dart)

Implementations of the [zed-pkg](https://github.com/zed-pkg) contract in Dart,
so a Flutter or web client answers "which version does `^1.2` install?" exactly
the way `zed-cli` does.

```dart
import 'package:zed_interfaces/package_metadata.dart';
import 'package:zed_lib/zed_lib.dart';

final metadata = PackageMetadata.fromJson(json);
try {
  final version = resolveVersion(metadata, '^1.2');   // '1.4.0', as published
} on ResolveException catch (error) {
  switch (error.kind) {
    case ResolveErrorKind.noVersions:        // nothing installable
    case ResolveErrorKind.invalidRequirement: // the requirement is the bug
    case ResolveErrorKind.unsatisfied:        // good requirement, no match
  }
}
```

Types come from `package:zed_interfaces`; this package adds only behavior.

## Not a binding, and not a dependency wrapper

This is a native Dart implementation of the same algebra, held to
[`conformance/cases/*.json`](../../conformance) alongside the Rust and
TypeScript slices. It is deliberately dependency-free: `pub_semver`'s dialect
differs from Cargo's exactly where it matters — a bare `1.0.0` is an *exact*
constraint there and a caret range here — and three implementations of one
contract cannot afford a translation layer whose edge cases nobody reads.

```sh
dart pub get
dart analyze
dart test        # runs the shared corpus
```

The package resolves `zed_interfaces` through a sibling checkout of
`zed-interfaces`, so clone both into the same parent directory.
