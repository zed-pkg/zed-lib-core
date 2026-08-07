// Implementations of the zed-pkg contract, in TypeScript.
//
// The TS slice of zed-lib exists so a browser client answers "which version
// does `^1.2` install?" exactly the way zed-cli does. Both run
// conformance/cases/*.json; a disagreement is a failing test rather than a
// support ticket.
//
// Types come from @zed-pkg/zed-interfaces; this package adds only behavior.

export { ResolveError, latestStable, resolveVersion, schemeOf } from "./resolve.ts";
export type { ResolveErrorKind } from "./resolve.ts";
export {
  compareVersions,
  isStable,
  looksLikeRange,
  normalizeCalver,
  parseRequirement,
  parseVersion,
  requirementMatches,
  resolve,
} from "./version.ts";
export type { Comparator, Op, Requirement, SemVer } from "./version.ts";
