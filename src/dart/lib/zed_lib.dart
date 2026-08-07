/// Implementations of the zed-pkg contract, in Dart.
///
/// The Dart slice of `zed-lib` exists so a Flutter or web client answers
/// "which version does `^1.2` install?" exactly the way `zed-cli` does. Both
/// run `conformance/cases/*.json`; a disagreement is a failing test rather than
/// a support ticket.
///
/// Types come from `package:zed_interfaces`; this package adds only behavior.
library;

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
