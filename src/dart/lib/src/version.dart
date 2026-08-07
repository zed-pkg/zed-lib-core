/// Version parsing, comparison, and requirement matching — the Dart mirror of
/// `zed_interfaces::version` (Rust).
///
/// Deliberately dependency-free. Reaching for `pub_semver` here would look
/// cheaper, but its dialect differs from Cargo's in exactly the places that
/// matter: a bare `1.0.0` is an *exact* constraint there and a caret range in
/// Cargo, and partial versions (`^1.2`) are spelled differently again. Three
/// implementations of one contract cannot afford a translation layer whose
/// edge cases nobody reads — so the algebra is written out, and
/// `conformance/cases/version-resolution.json` proves it agrees with Rust.
library;

/// A parsed semantic version. Build metadata is dropped on purpose: semver
/// precedence ignores it, so `2.0.0+incompatible` and `2.0.0` are one identity.
class SemVer implements Comparable<SemVer> {
  const SemVer(this.major, this.minor, this.patch, [this.pre = const []]);

  final int major;
  final int minor;
  final int patch;

  /// Dot-separated prerelease identifiers; empty for a stable release.
  final List<String> pre;

  bool get isStable => pre.isEmpty;

  @override
  int compareTo(SemVer other) {
    for (final pair in [
      [major, other.major],
      [minor, other.minor],
      [patch, other.patch],
    ]) {
      final ordering = pair[0].compareTo(pair[1]);
      if (ordering != 0) return ordering;
    }
    return _comparePre(pre, other.pre);
  }

  @override
  bool operator ==(Object other) => other is SemVer && compareTo(other) == 0;

  @override
  int get hashCode => Object.hash(major, minor, patch, pre.join('.'));

  @override
  String toString() {
    final core = '$major.$minor.$patch';
    return pre.isEmpty ? core : '$core-${pre.join('.')}';
  }
}

/// Semver §11: a prerelease is *lower* than the release it precedes; numeric
/// identifiers order numerically and rank below alphanumeric ones; when one set
/// is a prefix of the other, the longer set is higher.
int _comparePre(List<String> a, List<String> b) {
  if (a.isEmpty && b.isEmpty) return 0;
  if (a.isEmpty) return 1;
  if (b.isEmpty) return -1;
  for (var i = 0; i < a.length && i < b.length; i++) {
    final left = a[i];
    final right = b[i];
    final leftNum = int.tryParse(left);
    final rightNum = int.tryParse(right);
    final ordering = switch ((leftNum, rightNum)) {
      (final int l, final int r) => l.compareTo(r),
      (int(), null) => -1,
      (null, int()) => 1,
      _ => left.compareTo(right),
    };
    if (ordering != 0) return ordering;
  }
  return a.length.compareTo(b.length);
}

final RegExp _semverRe = RegExp(
  r'^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?(?:\+([0-9A-Za-z.-]+))?$',
);

SemVer? _parseStrict(String raw) {
  final match = _semverRe.firstMatch(raw);
  if (match == null) return null;
  final pre = match.group(4);
  // Leading zeros are illegal in semver numeric identifiers; calendar versions
  // hit this constantly (`2026.06.01`) and are normalized separately.
  for (final segment in [match.group(1)!, match.group(2)!, match.group(3)!]) {
    if (segment.length > 1 && segment.startsWith('0')) return null;
  }
  return SemVer(
    int.parse(match.group(1)!),
    int.parse(match.group(2)!),
    int.parse(match.group(3)!),
    pre == null || pre.isEmpty ? const [] : pre.split('.'),
  );
}

/// `2026.07.24` -> `2026.7.24`, `2026.07` -> `2026.7.0`, `2026` -> `2026.0.0`.
/// Leading zeros are stripped (semver forbids them) and 1–3 numeric segments are
/// padded. An optional `-prerelease` suffix survives. Null if not calendar-like.
String? normalizeCalver(String raw) {
  var input = raw.startsWith('v') ? raw.substring(1) : raw;
  String? pre;
  final dash = input.indexOf('-');
  if (dash > 0 && dash < input.length - 1) {
    pre = input.substring(dash + 1);
    input = input.substring(0, dash);
  }
  final parts = <int>[];
  for (final segment in input.split('.')) {
    if (segment.isEmpty || !segment.split('').every((c) => '0123456789'.contains(c))) {
      return null;
    }
    final value = int.tryParse(segment);
    if (value == null) return null;
    parts.add(value);
  }
  if (parts.isEmpty || parts.length > 3) return null;
  while (parts.length < 3) {
    parts.add(0);
  }
  final core = '${parts[0]}.${parts[1]}.${parts[2]}';
  return pre == null ? core : '$core-$pre';
}

/// Drop Go's `+incompatible` (and any build metadata) from a bare Go tag.
String? _normalizeGo(String raw) {
  final input = raw.startsWith('v') ? raw.substring(1) : raw;
  final core = input.split('+').first;
  return _parseStrict(core) == null ? null : core;
}

const _pep440Labels = <String, ({String label, bool prerelease})>{
  'a': (label: 'alpha', prerelease: true),
  'alpha': (label: 'alpha', prerelease: true),
  'b': (label: 'beta', prerelease: true),
  'beta': (label: 'beta', prerelease: true),
  'c': (label: 'rc', prerelease: true),
  'rc': (label: 'rc', prerelease: true),
  'pre': (label: 'rc', prerelease: true),
  'preview': (label: 'rc', prerelease: true),
  'dev': (label: 'dev', prerelease: true),
  'post': (label: 'post', prerelease: false),
  'rev': (label: 'post', prerelease: false),
  'r': (label: 'post', prerelease: false),
};

/// `1.2.3rc1` -> `1.2.3-rc.1`, `1.2a1` -> `1.2.0-alpha.1`, `1.2.3.post1` ->
/// `1.2.3+post.1` (semver has no "after the release" ordering). Conservative:
/// only the common shapes, null otherwise.
String? _normalizePep440(String raw) {
  final input = (raw.startsWith('v') ? raw.substring(1) : raw).toLowerCase();
  final marker = input.indexOf(RegExp('[a-z]'));
  if (marker < 0) return null;
  var release = input.substring(0, marker);
  final suffix = input.substring(marker);
  while (release.endsWith('.')) {
    release = release.substring(0, release.length - 1);
  }
  final nums = <int>[];
  for (final segment in release.split('.')) {
    if (segment.isEmpty) continue;
    final value = int.tryParse(segment);
    if (value == null) return null;
    nums.add(value);
  }
  if (nums.length > 3) return null;
  while (nums.length < 3) {
    nums.add(0);
  }
  final core = '${nums[0]}.${nums[1]}.${nums[2]}';

  final digit = suffix.indexOf(RegExp(r'\d'));
  final label = digit < 0 ? suffix : suffix.substring(0, digit);
  final number = digit < 0 ? '0' : suffix.substring(digit);
  if (int.tryParse(number) == null) return null;
  final mapped = _pep440Labels[label];
  if (mapped == null) return null;
  return mapped.prerelease
      ? '$core-${mapped.label}.$number'
      : '$core+${mapped.label}.$number';
}

/// Parse a published version string, tolerating the foreign spellings zed
/// federates: a bare `v` prefix, calendar versions, Go's `+incompatible`, and a
/// subset of PEP 440. Null for a genuinely opaque tag.
SemVer? parseVersion(String raw) {
  final direct = _parseStrict(raw);
  if (direct != null) return direct;
  if (raw.startsWith('v')) {
    final stripped = _parseStrict(raw.substring(1));
    if (stripped != null) return stripped;
  }
  for (final normalize in [_normalizeGo, normalizeCalver, _normalizePep440]) {
    final normalized = normalize(raw);
    if (normalized != null) {
      final parsed = _parseStrict(normalized.split('+').first);
      if (parsed != null) return parsed;
    }
  }
  return null;
}

/// One `<op><version>` bound.
class VersionBound {
  const VersionBound(this.op, this.version);

  final String op;
  final SemVer version;

  bool matches(SemVer candidate) {
    final ordering = candidate.compareTo(version);
    return switch (op) {
      '=' => ordering == 0,
      '>' => ordering > 0,
      '>=' => ordering >= 0,
      '<' => ordering < 0,
      '<=' => ordering <= 0,
      _ => false,
    };
  }

  @override
  String toString() => '$op$version';
}

/// A requirement is either a range (semver algebra) or an exact tag. An opaque
/// package's requirement is always the latter.
sealed class Requirement {
  const Requirement();

  /// Parse in **Cargo's dialect**: a bare `1.2.3` is a caret range, and only
  /// `=1.2.3` pins one version. Anything that is not a legible range becomes an
  /// exact tag, which is how opaque versions are requested.
  factory Requirement.parse(String input) {
    final comparators = _parseComparators(input);
    return comparators == null
        ? ExactRequirement(input)
        : RangeRequirement(comparators);
  }

  bool matches(String version);
}

class ExactRequirement extends Requirement {
  const ExactRequirement(this.tag);

  final String tag;

  @override
  bool matches(String version) => version == tag;
}

class RangeRequirement extends Requirement {
  const RangeRequirement(this.comparators);

  final List<VersionBound> comparators;

  @override
  bool matches(String version) {
    final parsed = parseVersion(version);
    if (parsed == null) return false;
    return comparators.every((c) => c.matches(parsed));
  }
}

/// Does this string *look* like a range? Used to tell a typo (`^1.x.y`) from a
/// legitimate opaque tag (`legacy-api`), which is the difference between an
/// invalid requirement and one that simply matches nothing.
bool looksLikeRange(String input) =>
    input.startsWith(RegExp(r'[\^~><=]')) ||
    input.contains('*') ||
    input.contains(',') ||
    input.trim().split(RegExp(r'\s+')).length > 1;

/// Split a numeric-ish version into 1–3 parts plus an optional prerelease, so
/// `1.2` and `2026` can expand into full bounds.
///
/// `*`, `x`, and `X` are all wildcards, and a wildcard must be the **last**
/// segment — `1.x.y` is an error, not `1.x`. That is the rule Cargo's `semver`
/// enforces ("unexpected character after wildcard"), and it is why `^1.x.y` is
/// an invalid requirement rather than a very wide one.
({List<int> parts, List<String> pre, bool wildcard})? _partial(String raw) {
  var input = raw.startsWith('v') ? raw.substring(1) : raw;
  input = input.split('+').first;
  List<String> pre = const [];
  final dash = input.indexOf('-');
  if (dash > 0 && dash < input.length - 1) {
    pre = input.substring(dash + 1).split('.');
    input = input.substring(0, dash);
  }
  if (input.isEmpty) return null;
  final parts = <int>[];
  var wildcard = false;
  final segments = input.split('.');
  for (var i = 0; i < segments.length; i++) {
    final segment = segments[i];
    if (segment == '*' || segment == 'x' || segment == 'X') {
      if (i != segments.length - 1 || parts.length >= 3) return null;
      wildcard = true;
      break;
    }
    // Semver forbids leading zeros in numeric identifiers, and Cargo's
    // requirement parser enforces it: `2026.07.24` is not a range at all, it is
    // an exact tag. Accepting it here would turn a calendar tag into a caret
    // range and, for an opaque package, into a spurious `invalid_requirement`.
    if (segment.length > 1 && segment.startsWith('0')) return null;
    final value = int.tryParse(segment);
    if (value == null) return null;
    parts.add(value);
  }
  if (parts.length > 3) return null;
  // A bare `*` constrains nothing; anything else needs at least one number.
  if (parts.isEmpty && !wildcard) return null;
  return (parts: parts, pre: pre, wildcard: wildcard);
}

SemVer _atLeast(List<int> parts, List<String> pre) => SemVer(
  parts[0],
  parts.length > 1 ? parts[1] : 0,
  parts.length > 2 ? parts[2] : 0,
  pre,
);

/// Cargo's caret: the upper bound is the next version that changes the
/// left-most non-zero segment. `^0.2.3` is `<0.3.0`, `^0.0.3` is `<0.0.4`.
SemVer _caretUpper(List<int> parts) {
  if (parts[0] != 0) return SemVer(parts[0] + 1, 0, 0);
  if (parts.length == 1) return const SemVer(1, 0, 0);
  if (parts[1] != 0) return SemVer(0, parts[1] + 1, 0);
  if (parts.length == 2) return const SemVer(0, 1, 0);
  if (parts[2] != 0) return SemVer(0, 0, parts[2] + 1);
  return const SemVer(0, 0, 1);
}

/// Cargo's tilde: only the last specified segment may grow.
SemVer _tildeUpper(List<int> parts) => switch (parts.length) {
  1 => SemVer(parts[0] + 1, 0, 0),
  _ => SemVer(parts[0], parts[1] + 1, 0),
};

List<VersionBound>? _parseComparators(String input) {
  final trimmed = input.trim();
  if (trimmed.isEmpty) return null;

  // `>= 1.0.0, < 2.0.0` is legal — Cargo allows space between an operator and
  // its version. Glue them back together before splitting, or the operator
  // becomes its own token and the whole requirement reads as an opaque tag.
  final glued = trimmed.replaceAllMapped(
    RegExp(r'(\^|~|>=|<=|>|<|=)\s+'),
    (match) => match[1]!,
  );

  final comparators = <VersionBound>[];
  var sawToken = false;
  for (final token in glued.split(RegExp(r'\s*,\s*|\s+'))) {
    if (token.isEmpty) continue;
    final expanded = _expand(token);
    if (expanded == null) return null;
    sawToken = true;
    comparators.addAll(expanded);
  }
  // `*` is a legitimate range that constrains nothing, so emptiness cannot
  // double as the failure signal.
  return sawToken ? comparators : null;
}

List<VersionBound>? _expand(String token) {
  final match = RegExp(r'^(\^|~|>=|<=|>|<|=)?\s*(.+)$').firstMatch(token);
  if (match == null) return null;
  final explicitOp = match.group(1);
  final op = explicitOp ?? '^'; // Cargo: a bare version is a caret range.
  final rest = match.group(2)!.trim();
  final partial = _partial(rest);
  if (partial == null) return null;
  final parts = partial.parts;
  final pre = partial.pre;

  if (parts.isEmpty) return const []; // bare `*`/`x`: matches anything

  // An explicit wildcard binds tighter than an omitted segment: `1.2` is
  // `^1.2` (< 2.0.0), but `1.2.*` is the 1.2 line (< 1.3.0). With an operator
  // the wildcard is just the segments the author left off (`^1.*` == `^1`).
  if (partial.wildcard && explicitOp == null) {
    return [
      VersionBound('>=', _atLeast(parts, pre)),
      VersionBound('<', _tildeUpper(parts)),
    ];
  }

  switch (op) {
    case '^':
      return [
        VersionBound('>=', _atLeast(parts, pre)),
        VersionBound('<', _caretUpper(parts)),
      ];
    case '~':
      return [
        VersionBound('>=', _atLeast(parts, pre)),
        VersionBound('<', _tildeUpper(parts)),
      ];
    case '=':
      // `=1.2` is not "exactly 1.2.0" in Cargo, it is the 1.2 line.
      if (parts.length == 3) return [VersionBound('=', _atLeast(parts, pre))];
      return [
        VersionBound('>=', _atLeast(parts, pre)),
        VersionBound('<', _tildeUpper(parts)),
      ];
    default:
      return [VersionBound(op, _atLeast(parts, pre))];
  }
}

/// Pick the version satisfying `requirement`, returned in its **published
/// spelling** so the store address and VCS tag stay faithful to the tag the
/// publisher pushed. Prereleases never satisfy a range.
String? resolveRequirement(Requirement requirement, List<String> versions) {
  switch (requirement) {
    case ExactRequirement(:final tag):
      for (final version in versions) {
        if (version == tag) return version;
      }
      return null;
    case RangeRequirement():
      String? best;
      SemVer? bestParsed;
      for (final version in versions) {
        final parsed = parseVersion(version);
        if (parsed == null || !parsed.isStable) continue;
        if (!requirement.matches(version)) continue;
        // `>= 0`, not `> 0`: Rust resolves with `Iterator::max_by`, which
        // returns the LAST maximum. Distinct spellings can parse to the same
        // version (`1.2.3` and `1.2.3.post1`, `1.0.0` and `v1.0.0`), so the
        // tie-break decides which spelling is installed.
        if (bestParsed == null || parsed.compareTo(bestParsed) >= 0) {
          best = version;
          bestParsed = parsed;
        }
      }
      return best;
  }
}
