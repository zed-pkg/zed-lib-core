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
export 'src/version.dart'
    show
        Comparator,
        ExactRequirement,
        RangeRequirement,
        Requirement,
        SemVer,
        looksLikeRange,
        normalizeCalver,
        parseVersion,
        resolve;
