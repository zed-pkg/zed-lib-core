/// Scheme-aware resolution against registry metadata — the Dart mirror of
/// `zed_lib::resolve` (Rust), verified against the same corpus.
library;

import 'package:zed_interfaces/package_metadata.dart';

import 'version.dart';

/// Why a requirement did not resolve. The three cases need different fixes, so
/// they stay distinct rather than collapsing into null.
enum ResolveErrorKind {
  /// The registry knows the package but it has no installable versions.
  noVersions('no_versions'),

  /// The requirement cannot mean anything for this package's scheme.
  invalidRequirement('invalid_requirement'),

  /// A well-formed requirement that nothing published satisfies.
  unsatisfied('unsatisfied');

  const ResolveErrorKind(this.wire);

  /// Stable string shared with the conformance corpus and the other
  /// implementations. Renaming one breaks every consumer that matches on it.
  final String wire;
}

class ResolveException implements Exception {
  ResolveException(this.kind, this.message);

  final ResolveErrorKind kind;
  final String message;

  @override
  String toString() => message;
}

/// The scheme a package declared, defaulting to semver.
///
/// `PackageMetadata.versionScheme` is nullable because the field carries a
/// serde default and the server omits it for the common case — absent means
/// semver, not "unknown".
VersionScheme schemeOf(PackageMetadata metadata) =>
    metadata.versionScheme ?? VersionScheme.semver;

/// Resolve `requirement` against what the registry says a package published.
///
/// Returns the version in its published spelling. Under [VersionScheme.opaque]
/// the requirement must be an exact tag: those packages have no ordering, so
/// `^1.0` is not "everything 1.x", it is a mistake that would otherwise resolve
/// through semver algebra by accident.
String resolveVersion(PackageMetadata metadata, String requirement) {
  final id = '${metadata.org}/${metadata.name}';

  if (metadata.versions.isEmpty) {
    throw ResolveException(
      ResolveErrorKind.noVersions,
      '$id has no published versions',
    );
  }

  final scheme = schemeOf(metadata);
  final parsed = Requirement.parse(requirement);

  if (scheme == VersionScheme.opaque && parsed is RangeRequirement) {
    throw ResolveException(
      ResolveErrorKind.invalidRequirement,
      '`$requirement` is not a valid requirement for $id: opaque-versioned '
      'packages have no range algebra; require an exact tag',
    );
  }

  // A range that *looks* like one but does not parse (`^1.x.y`) would degrade
  // into an exact tag and never match. Catch it as the typo it is.
  if (scheme != VersionScheme.opaque &&
      parsed is ExactRequirement &&
      looksLikeRange(requirement)) {
    throw ResolveException(
      ResolveErrorKind.invalidRequirement,
      '`$requirement` is not a valid requirement for $id: looks like a version '
      'range but is not a valid one',
    );
  }

  final resolved = resolve(parsed, metadata.versions);
  if (resolved == null) {
    throw ResolveException(
      ResolveErrorKind.unsatisfied,
      '$id has no version matching `$requirement`; '
      'published: ${metadata.versions.join(', ')}',
    );
  }
  return resolved;
}

/// The newest installable version, ignoring prereleases.
///
/// `PackageMetadata.latest` is what the registry computed when the package was
/// last published and can lag a yank, so this recomputes from the version list
/// the same response carried.
String? latestStable(PackageMetadata metadata) {
  if (schemeOf(metadata) == VersionScheme.opaque) {
    final latest = metadata.latest;
    if (latest == null) return null;
    return metadata.versions.contains(latest) ? latest : null;
  }
  String? best;
  SemVer? bestParsed;
  for (final version in metadata.versions) {
    final parsed = parseVersion(version);
    if (parsed == null || !parsed.isStable) continue;
    if (bestParsed == null || parsed.compareTo(bestParsed) > 0) {
      best = version;
      bestParsed = parsed;
    }
  }
  return best;
}
