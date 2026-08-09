// Implementations of the zed-pkg contract, in TypeScript.
//
// The TS slice of zed-lib exists so a browser client answers planning and
// resolution questions exactly the way zed-cli does. All slices run the same
// conformance corpus; a disagreement is a failing test rather than a support
// ticket.
//
// Types come from @zed-pkg/zed-interfaces; this package adds only behavior.

export {
  planRegistryNamespaces,
  summarizeRegistryNamespacePlan,
} from "./namespace-plan.ts";
export type {
  RegistryNamespacePlanProof,
  RegistryNamespacePlanSummary,
} from "./namespace-plan.ts";
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
  resolveRequirement,
} from "./version.ts";
export type { Op, Requirement, SemVer, VersionBound } from "./version.ts";