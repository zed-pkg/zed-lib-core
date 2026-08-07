// Scheme-aware resolution against registry metadata — the TypeScript mirror of
// `zed_lib::resolve` (Rust), verified against the same corpus.

import type { PackageMetadata, VersionScheme } from "@zed-pkg/zed-interfaces";

import { compareVersions, isStable, looksLikeRange, parseRequirement, parseVersion, resolveRequirement } from "./version.ts";

/** Stable strings shared with the conformance corpus and the other
 *  implementations. Renaming one breaks every consumer that matches on it. */
export type ResolveErrorKind = "no_versions" | "invalid_requirement" | "unsatisfied";

/** Why a requirement did not resolve. The three cases need different fixes, so
 *  they stay distinct rather than collapsing into null. */
export class ResolveError extends Error {
  // Declared as a field rather than a constructor parameter property: those
  // are not erasable, and this package is consumed straight from source by
  // Node's type-stripping and by bundlers.
  readonly kind: ResolveErrorKind;

  constructor(kind: ResolveErrorKind, message: string) {
    super(message);
    this.name = "ResolveError";
    this.kind = kind;
  }
}

/** The scheme a package declared, defaulting to semver.
 *
 *  `version_scheme` is optional on the wire because the field carries a serde
 *  default and the server omits it for the common case — absent means semver,
 *  not "unknown". */
export function schemeOf(metadata: PackageMetadata): VersionScheme {
  return metadata.version_scheme ?? "semver";
}

/** Resolve `requirement` against what the registry says a package published.
 *
 *  Returns the version in its published spelling. Under the `opaque` scheme the
 *  requirement must be an exact tag: those packages have no ordering, so `^1.0`
 *  is not "everything 1.x", it is a mistake that would otherwise resolve
 *  through semver algebra by accident. */
export function resolveVersion(metadata: PackageMetadata, requirement: string): string {
  const id = `${metadata.org}/${metadata.name}`;

  if (metadata.versions.length === 0) {
    throw new ResolveError("no_versions", `${id} has no published versions`);
  }

  const scheme = schemeOf(metadata);
  const parsed = parseRequirement(requirement);

  if (scheme === "opaque" && parsed.kind === "range") {
    throw new ResolveError(
      "invalid_requirement",
      `\`${requirement}\` is not a valid requirement for ${id}: opaque-versioned ` +
        `packages have no range algebra; require an exact tag`,
    );
  }

  // A range that *looks* like one but does not parse (`^1.x.y`) would degrade
  // into an exact tag and never match. Catch it as the typo it is.
  if (scheme !== "opaque" && parsed.kind === "exact" && looksLikeRange(requirement)) {
    throw new ResolveError(
      "invalid_requirement",
      `\`${requirement}\` is not a valid requirement for ${id}: looks like a version ` +
        `range but is not a valid one`,
    );
  }

  const resolved = resolveRequirement(parsed, metadata.versions);
  if (resolved === null) {
    throw new ResolveError(
      "unsatisfied",
      `${id} has no version matching \`${requirement}\`; published: ${metadata.versions.join(", ")}`,
    );
  }
  return resolved;
}

/** The newest installable version, ignoring prereleases.
 *
 *  `latest` is what the registry computed when the package was last published
 *  and can lag a yank, so this recomputes from the version list the same
 *  response carried. */
export function latestStable(metadata: PackageMetadata): string | null {
  if (schemeOf(metadata) === "opaque") {
    const latest = metadata.latest;
    if (!latest) return null;
    return metadata.versions.includes(latest) ? latest : null;
  }
  let best: string | null = null;
  let bestParsed: ReturnType<typeof parseVersion> = null;
  for (const version of metadata.versions) {
    const parsed = parseVersion(version);
    if (!parsed || !isStable(parsed)) continue;
    // Ties go to the last equal element, matching Rust's `Iterator::max_by`.
    if (bestParsed === null || compareVersions(parsed, bestParsed) >= 0) {
      best = version;
      bestParsed = parsed;
    }
  }
  return best;
}
