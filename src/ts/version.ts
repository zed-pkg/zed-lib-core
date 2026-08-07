// Version parsing, comparison, and requirement matching — the TypeScript
// mirror of `zed_interfaces::version` (Rust).
//
// Deliberately dependency-free. The npm `semver` package looks like the
// obvious answer, but its dialect differs from Cargo's exactly where it hurts:
// a bare `1.0.0` is an *exact* constraint there and a caret range in Cargo, and
// `1.2` means the 1.2 line rather than `^1.2`. Three implementations of one
// contract cannot afford a translation layer whose edge cases nobody reads —
// so the algebra is written out, and the shared corpus proves it agrees.

/** A parsed semantic version. Build metadata is dropped: semver precedence
 *  ignores it, so `2.0.0+incompatible` and `2.0.0` are one identity. */
export interface SemVer {
  readonly major: number;
  readonly minor: number;
  readonly patch: number;
  /** Dot-separated prerelease identifiers; empty for a stable release. */
  readonly pre: readonly string[];
}

export const isStable = (version: SemVer): boolean => version.pre.length === 0;

/** Semver §11: a prerelease is lower than the release it precedes; numeric
 *  identifiers order numerically and rank below alphanumeric ones; when one set
 *  is a prefix of the other, the longer set is higher. */
function comparePre(a: readonly string[], b: readonly string[]): number {
  if (a.length === 0 && b.length === 0) return 0;
  if (a.length === 0) return 1;
  if (b.length === 0) return -1;
  for (let i = 0; i < a.length && i < b.length; i++) {
    const left = a[i] as string;
    const right = b[i] as string;
    const leftNum = /^\d+$/.test(left) ? Number(left) : null;
    const rightNum = /^\d+$/.test(right) ? Number(right) : null;
    let ordering: number;
    if (leftNum !== null && rightNum !== null) ordering = leftNum - rightNum;
    else if (leftNum !== null) ordering = -1;
    else if (rightNum !== null) ordering = 1;
    else ordering = left < right ? -1 : left > right ? 1 : 0;
    if (ordering !== 0) return ordering;
  }
  return a.length - b.length;
}

export function compareVersions(a: SemVer, b: SemVer): number {
  return (
    a.major - b.major ||
    a.minor - b.minor ||
    a.patch - b.patch ||
    comparePre(a.pre, b.pre)
  );
}

const SEMVER_RE = /^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?(?:\+([0-9A-Za-z.-]+))?$/;

function parseStrict(raw: string): SemVer | null {
  const match = SEMVER_RE.exec(raw);
  if (!match) return null;
  const [, major, minor, patch, pre] = match as unknown as [string, string, string, string, string | undefined];
  // Leading zeros are illegal in semver numeric identifiers; calendar versions
  // hit this constantly (`2026.06.01`) and are normalized separately.
  for (const segment of [major, minor, patch]) {
    if (segment.length > 1 && segment.startsWith("0")) return null;
  }
  return {
    major: Number(major),
    minor: Number(minor),
    patch: Number(patch),
    pre: pre ? pre.split(".") : [],
  };
}

/** `2026.07.24` -> `2026.7.24`, `2026.07` -> `2026.7.0`, `2026` -> `2026.0.0`.
 *  Leading zeros stripped (semver forbids them), 1–3 numeric segments padded,
 *  an optional `-prerelease` suffix preserved. Null if not calendar-like. */
export function normalizeCalver(raw: string): string | null {
  let input = raw.startsWith("v") ? raw.slice(1) : raw;
  let pre: string | null = null;
  const dash = input.indexOf("-");
  if (dash > 0 && dash < input.length - 1) {
    pre = input.slice(dash + 1);
    input = input.slice(0, dash);
  }
  const parts: number[] = [];
  for (const segment of input.split(".")) {
    if (!/^\d+$/.test(segment)) return null;
    parts.push(Number(segment));
  }
  if (parts.length === 0 || parts.length > 3) return null;
  while (parts.length < 3) parts.push(0);
  const core = `${parts[0]}.${parts[1]}.${parts[2]}`;
  return pre === null ? core : `${core}-${pre}`;
}

/** Drop Go's `+incompatible` (and any build metadata) from a bare Go tag. */
function normalizeGo(raw: string): string | null {
  const input = raw.startsWith("v") ? raw.slice(1) : raw;
  const core = input.split("+")[0] as string;
  return parseStrict(core) === null ? null : core;
}

const PEP440_LABELS: Record<string, { label: string; prerelease: boolean }> = {
  a: { label: "alpha", prerelease: true },
  alpha: { label: "alpha", prerelease: true },
  b: { label: "beta", prerelease: true },
  beta: { label: "beta", prerelease: true },
  c: { label: "rc", prerelease: true },
  rc: { label: "rc", prerelease: true },
  pre: { label: "rc", prerelease: true },
  preview: { label: "rc", prerelease: true },
  dev: { label: "dev", prerelease: true },
  post: { label: "post", prerelease: false },
  rev: { label: "post", prerelease: false },
  r: { label: "post", prerelease: false },
};

/** `1.2.3rc1` -> `1.2.3-rc.1`, `1.2a1` -> `1.2.0-alpha.1`, `1.2.3.post1` ->
 *  `1.2.3+post.1` (semver has no "after the release" ordering). Conservative:
 *  only the common shapes, null otherwise. */
function normalizePep440(raw: string): string | null {
  const input = (raw.startsWith("v") ? raw.slice(1) : raw).toLowerCase();
  const marker = input.search(/[a-z]/);
  if (marker < 0) return null;
  const release = input.slice(0, marker).replace(/\.+$/, "");
  const suffix = input.slice(marker);
  const nums: number[] = [];
  for (const segment of release.split(".")) {
    if (segment === "") continue;
    if (!/^\d+$/.test(segment)) return null;
    nums.push(Number(segment));
  }
  if (nums.length > 3) return null;
  while (nums.length < 3) nums.push(0);
  const core = `${nums[0]}.${nums[1]}.${nums[2]}`;

  const digit = suffix.search(/\d/);
  const label = digit < 0 ? suffix : suffix.slice(0, digit);
  const num = digit < 0 ? "0" : suffix.slice(digit);
  if (!/^\d+$/.test(num)) return null;
  const mapped = PEP440_LABELS[label];
  if (!mapped) return null;
  return mapped.prerelease ? `${core}-${mapped.label}.${num}` : `${core}+${mapped.label}.${num}`;
}

/** Parse a published version string, tolerating the foreign spellings zed
 *  federates: a bare `v` prefix, calendar versions, Go's `+incompatible`, and a
 *  subset of PEP 440. Null for a genuinely opaque tag. */
export function parseVersion(raw: string): SemVer | null {
  const direct = parseStrict(raw);
  if (direct) return direct;
  if (raw.startsWith("v")) {
    const stripped = parseStrict(raw.slice(1));
    if (stripped) return stripped;
  }
  for (const normalize of [normalizeGo, normalizeCalver, normalizePep440]) {
    const normalized = normalize(raw);
    if (normalized !== null) {
      const parsed = parseStrict(normalized.split("+")[0] as string);
      if (parsed) return parsed;
    }
  }
  return null;
}

export type Op = "=" | ">" | ">=" | "<" | "<=";

/** One `<op><version>` bound. */
export interface VersionBound {
  readonly op: Op;
  readonly version: SemVer;
}

function matchesBound(bound: VersionBound, candidate: SemVer): boolean {
  const ordering = compareVersions(candidate, bound.version);
  switch (bound.op) {
    case "=":
      return ordering === 0;
    case ">":
      return ordering > 0;
    case ">=":
      return ordering >= 0;
    case "<":
      return ordering < 0;
    case "<=":
      return ordering <= 0;
  }
}

/** A requirement is either a range (semver algebra) or an exact tag. An opaque
 *  package's requirement is always the latter. */
export type Requirement =
  | { readonly kind: "range"; readonly comparators: readonly VersionBound[] }
  | { readonly kind: "exact"; readonly tag: string };

/** Does this string *look* like a range? Tells a typo (`^1.x.y`) from a
 *  legitimate opaque tag (`legacy-api`) — the difference between an invalid
 *  requirement and one that simply matches nothing. */
export function looksLikeRange(input: string): boolean {
  return (
    /^[\^~><=]/.test(input) ||
    input.includes("*") ||
    input.includes(",") ||
    input.trim().split(/\s+/).length > 1
  );
}

interface Partial {
  readonly parts: readonly number[];
  readonly pre: readonly string[];
  readonly wildcard: boolean;
}

/** Split a numeric-ish version into 1–3 parts plus an optional prerelease.
 *
 *  `*`, `x`, and `X` are all wildcards, and a wildcard must be the **last**
 *  segment — `1.x.y` is an error, not `1.x`. That is the rule Cargo's `semver`
 *  enforces ("unexpected character after wildcard"), and it is why `^1.x.y` is
 *  an invalid requirement rather than a very wide one. */
function partial(raw: string): Partial | null {
  let input = raw.startsWith("v") ? raw.slice(1) : raw;
  input = input.split("+")[0] as string;
  let pre: string[] = [];
  const dash = input.indexOf("-");
  if (dash > 0 && dash < input.length - 1) {
    pre = input.slice(dash + 1).split(".");
    input = input.slice(0, dash);
  }
  if (input === "") return null;
  const parts: number[] = [];
  let wildcard = false;
  const segments = input.split(".");
  for (let i = 0; i < segments.length; i++) {
    const segment = segments[i] as string;
    if (segment === "*" || segment === "x" || segment === "X") {
      if (i !== segments.length - 1 || parts.length >= 3) return null;
      wildcard = true;
      break;
    }
    // Semver forbids leading zeros in numeric identifiers, and Cargo's
    // requirement parser enforces it: `2026.07.24` is not a range at all, it is
    // an exact tag. Accepting it here would turn a calendar tag into a caret
    // range and, for an opaque package, into a spurious `invalid_requirement`.
    if (!/^\d+$/.test(segment) || (segment.length > 1 && segment.startsWith("0"))) return null;
    parts.push(Number(segment));
  }
  if (parts.length > 3) return null;
  // A bare `*` constrains nothing; anything else needs at least one number.
  if (parts.length === 0 && !wildcard) return null;
  return { parts, pre, wildcard };
}

const atLeast = (parts: readonly number[], pre: readonly string[]): SemVer => ({
  major: parts[0] as number,
  minor: parts.length > 1 ? (parts[1] as number) : 0,
  patch: parts.length > 2 ? (parts[2] as number) : 0,
  pre,
});

const at = (major: number, minor: number, patch: number): SemVer => ({
  major,
  minor,
  patch,
  pre: [],
});

/** Cargo's caret: the upper bound is the next version that changes the
 *  left-most non-zero segment. `^0.2.3` is `<0.3.0`, `^0.0.3` is `<0.0.4`. */
function caretUpper(parts: readonly number[]): SemVer {
  const [major, minor, patch] = parts as [number, number?, number?];
  if (major !== 0) return at(major + 1, 0, 0);
  if (minor === undefined) return at(1, 0, 0);
  if (minor !== 0) return at(0, minor + 1, 0);
  if (patch === undefined) return at(0, 1, 0);
  if (patch !== 0) return at(0, 0, patch + 1);
  return at(0, 0, 1);
}

/** Cargo's tilde: only the last specified segment may grow. */
function tildeUpper(parts: readonly number[]): SemVer {
  const [major, minor] = parts as [number, number?];
  return minor === undefined ? at(major + 1, 0, 0) : at(major, minor + 1, 0);
}

const OP_RE = /^(\^|~|>=|<=|>|<|=)?\s*(.+)$/;

function expand(token: string): VersionBound[] | null {
  const match = OP_RE.exec(token);
  if (!match) return null;
  const explicitOp = match[1];
  const rest = (match[2] as string).trim();
  const split = partial(rest);
  if (!split) return null;
  const { parts, pre } = split;

  if (parts.length === 0) return []; // bare `*`/`x`: matches anything

  // An explicit wildcard binds tighter than an omitted segment: `1.2` is
  // `^1.2` (< 2.0.0), but `1.2.*` is the 1.2 line (< 1.3.0). With an operator
  // the wildcard is just the segments the author left off (`^1.*` == `^1`).
  if (split.wildcard && explicitOp === undefined) {
    return [
      { op: ">=", version: atLeast(parts, pre) },
      { op: "<", version: tildeUpper(parts) },
    ];
  }

  switch (explicitOp ?? "^") { // Cargo: a bare version is a caret range.
    case "^":
      return [
        { op: ">=", version: atLeast(parts, pre) },
        { op: "<", version: caretUpper(parts) },
      ];
    case "~":
      return [
        { op: ">=", version: atLeast(parts, pre) },
        { op: "<", version: tildeUpper(parts) },
      ];
    case "=":
      // `=1.2` is not "exactly 1.2.0" in Cargo, it is the 1.2 line.
      if (parts.length === 3) return [{ op: "=", version: atLeast(parts, pre) }];
      return [
        { op: ">=", version: atLeast(parts, pre) },
        { op: "<", version: tildeUpper(parts) },
      ];
    default:
      return [{ op: explicitOp as Op, version: atLeast(parts, pre) }];
  }
}

function parseBounds(input: string): VersionBound[] | null {
  const trimmed = input.trim();
  if (trimmed === "") return null;
  // `>= 1.0.0, < 2.0.0` is legal — Cargo allows space between an operator and
  // its version. Glue them back together before splitting, or the operator
  // becomes its own token and the whole requirement reads as an opaque tag.
  const glued = trimmed.replace(/(\^|~|>=|<=|>|<|=)\s+/g, "$1");
  const comparators: VersionBound[] = [];
  let sawToken = false;
  for (const token of glued.split(/\s*,\s*|\s+/)) {
    if (token === "") continue;
    const expanded = expand(token);
    if (!expanded) return null;
    sawToken = true;
    comparators.push(...expanded);
  }
  // `*` is a legitimate range that constrains nothing, so emptiness cannot
  // double as the failure signal.
  return sawToken ? comparators : null;
}

/** Parse in **Cargo's dialect**: a bare `1.2.3` is a caret range, and only
 *  `=1.2.3` pins one version. Anything that is not a legible range becomes an
 *  exact tag, which is how opaque versions are requested. */
export function parseRequirement(input: string): Requirement {
  const comparators = parseBounds(input);
  return comparators === null
    ? { kind: "exact", tag: input }
    : { kind: "range", comparators };
}

export function requirementMatches(requirement: Requirement, version: string): boolean {
  if (requirement.kind === "exact") return version === requirement.tag;
  const parsed = parseVersion(version);
  if (!parsed) return false;
  return requirement.comparators.every((bound) => matchesBound(bound, parsed));
}

/** Pick the version satisfying `requirement`, returned in its **published
 *  spelling** so the store address and VCS tag stay faithful to the tag the
 *  publisher pushed. Prereleases never satisfy a range. */
export function resolveRequirement(
  requirement: Requirement,
  versions: readonly string[],
): string | null {
  if (requirement.kind === "exact") {
    return versions.find((version) => version === requirement.tag) ?? null;
  }
  let best: string | null = null;
  let bestParsed: SemVer | null = null;
  for (const version of versions) {
    const parsed = parseVersion(version);
    if (!parsed || !isStable(parsed)) continue;
    if (!requirementMatches(requirement, version)) continue;
    // `>= 0`, not `> 0`: Rust resolves with `Iterator::max_by`, which returns
    // the LAST maximum. Distinct spellings can parse to the same version
    // (`1.2.3` and `1.2.3.post1`, `1.0.0` and `v1.0.0`), so the tie-break
    // decides which spelling is installed.
    if (bestParsed === null || compareVersions(parsed, bestParsed) >= 0) {
      best = version;
      bestParsed = parsed;
    }
  }
  return best;
}
