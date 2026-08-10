/// Implementations of the zed-pkg contract, in Dart.
///
/// The Dart slice of `zed-lib` exists so Flutter and web clients answer
/// planning and resolution questions exactly the way `zed-cli` does. All
/// slices run the shared conformance corpus.
///
/// Types come from `package:zed_interfaces`; this package adds only behavior.
library;

export 'src/namespace_plan.dart'
    show planRegistryNamespaces, summarizeRegistryNamespacePlan;
export 'src/resolve.dart'
    show ResolveErrorKind, ResolveException, latestStable, resolveVersion, schemeOf;
// `VersionBound`, not `Comparator`: dart:core exports `Comparator<T>`, and an
// explicit import *wins* over the implicit dart:core one — so the old name did
// not raise an ambiguity error, it silently shadowed the real type and broke
// unrelated consumer code. `resolveRequirement`, not `resolve`: a top-level
// `resolve` next to `resolveVersion` reads as a typo of it.
export 'src/version.dart'
    show
        ExactRequirement,
        RangeRequirement,
        Requirement,
        SemVer,
        VersionBound,
        looksLikeRange,
        normalizeCalver,
        parseVersion,
        resolveRequirement;