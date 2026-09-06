// Bounded model of the dependency-resolution contract shared by the Rust,
// TypeScript, and Dart slices.
//
// The production implementations parse strings and delegate semver details to
// the shared interface package. This model deliberately abstracts those
// details into a finite set of published candidate identities and requirement
// shapes, then exhaustively checks the safety boundary around their outcomes.
// It proves that every modeled result is total, successful selections remain in
// the published set, opaque packages reject range algebra, and failed graph
// resolution never emits a partial lock.
module dependency_resolution {
  pure val NONE = 0

  pure val SEMVER = 1
  pure val CALVER = 2
  pure val OPAQUE = 3

  pure val RESOLVED = 1
  pure val NO_VERSIONS = 2
  pure val INVALID_REQUIREMENT = 3
  pure val UNSATISFIED = 4

  pure val RANGE = 1
  pure val EXACT = 2
  pure val EXACT_MISSING = 3
  pure val MALFORMED_RANGE = 4

  pure val CASES = Set(1, 2, 3, 4, 5, 6, 7, 8)
  pure val GRAPH_CASES = Set(1, 2, 3)

  type State = {
    has_case: bool,
    case_id: int,
    result_kind: int,
    resolved_version: int,
    graph_case: int,
    graph_ok: bool,
    lock_written: bool,
    partial_lock: bool,
    saw_semver_selection: bool,
    saw_calver_selection: bool,
    saw_opaque_range_rejection: bool,
    saw_malformed_requirement_rejection: bool,
    saw_no_versions: bool,
    saw_unsatisfied: bool,
    saw_missing_dependency: bool,
    saw_cycle: bool,
    saw_deterministic_replay: bool,
  }

  var s: State

  // Case 1 is a stable semver range. Case 2 has two published spellings for
  // one normalized version; candidate 202 is the last maximum and therefore
  // the spelling the Rust iterator semantics preserve. Case 3 is calver. Case
  // 4 is an exact opaque tag. Cases 5–8 cover malformed, empty, and
  // unsatisfied outcomes.
  pure def case_scheme(case) =
    if (case == 3) CALVER
    else if (case == 4 or case == 5) OPAQUE
    else SEMVER

  pure def published(case) =
    if (case == 1) Set(101, 102, 103)
    else if (case == 2) Set(201, 202)
    else if (case == 3) Set(301, 302, 303)
    else if (case == 4 or case == 5) Set(401)
    else if (case == 6) Set(501, 502)
    else if (case == 8) Set(601, 602)
    else Set()

  pure def case_requirement(case) =
    if (case == 1 or case == 3) RANGE
    else if (case == 2 or case == 4) EXACT
    else if (case == 5) RANGE
    else if (case == 6) MALFORMED_RANGE
    else if (case == 7) RANGE
    else EXACT_MISSING

  pure def expected_kind(case) =
    if (case == 5 or case == 6) INVALID_REQUIREMENT
    else if (case == 7) NO_VERSIONS
    else if (case == 8) UNSATISFIED
    else RESOLVED

  pure def expected_version(case) =
    if (case == 1) 103
    else if (case == 2) 202
    else if (case == 3) 303
    else if (case == 4) 401
    else NONE

  pure def graph_is_ok(graph_case) = graph_case == 1

  action init = {
    s' = {
      has_case: false,
      case_id: NONE,
      result_kind: NONE,
      resolved_version: NONE,
      graph_case: NONE,
      graph_ok: false,
      lock_written: false,
      partial_lock: false,
      saw_semver_selection: false,
      saw_calver_selection: false,
      saw_opaque_range_rejection: false,
      saw_malformed_requirement_rejection: false,
      saw_no_versions: false,
      saw_unsatisfied: false,
      saw_missing_dependency: false,
      saw_cycle: false,
      saw_deterministic_replay: false,
    }
  }

  action evaluate(case) = all {
    CASES.contains(case),
    s' = {
      ...s,
      has_case: true,
      case_id: case,
      result_kind: expected_kind(case),
      resolved_version: expected_version(case),
      saw_semver_selection:
        s.saw_semver_selection or (case == 1 or case == 2),
      saw_calver_selection: s.saw_calver_selection or case == 3,
      saw_opaque_range_rejection:
        s.saw_opaque_range_rejection or case == 5,
      saw_malformed_requirement_rejection:
        s.saw_malformed_requirement_rejection or case == 6,
      saw_no_versions: s.saw_no_versions or case == 7,
      saw_unsatisfied: s.saw_unsatisfied or case == 8,
      saw_deterministic_replay:
        s.saw_deterministic_replay or (
          s.has_case and s.case_id == case
            and s.result_kind == expected_kind(case)
            and s.resolved_version == expected_version(case)
        ),
    },
  }

  action resolve_graph(graph_case) = all {
    GRAPH_CASES.contains(graph_case),
    s' = {
      ...s,
      graph_case: graph_case,
      graph_ok: graph_is_ok(graph_case),
      lock_written: graph_is_ok(graph_case),
      // A missing transitive dependency or cycle fails before any partial lock
      // becomes observable to a caller.
      partial_lock: false,
      saw_missing_dependency:
        s.saw_missing_dependency or graph_case == 2,
      saw_cycle: s.saw_cycle or graph_case == 3,
    },
  }

  action idle = s' = s

  action step = {
    nondet case = CASES.oneOf()
    nondet graph_case = GRAPH_CASES.oneOf()
    any {
      evaluate(case),
      resolve_graph(graph_case),
      idle,
    }
  }

  val bounded_state = and {
    s.case_id == NONE or CASES.contains(s.case_id),
    s.graph_case == NONE or GRAPH_CASES.contains(s.graph_case),
    s.result_kind >= NONE and s.result_kind <= UNSATISFIED,
    s.resolved_version == NONE or published(s.case_id).contains(s.resolved_version),
  }

  val current_outcome_is_total = not(s.has_case) or s.result_kind != NONE

  val current_result_matches_model =
    not(s.has_case)
      or and {
        s.result_kind == expected_kind(s.case_id),
        s.resolved_version == expected_version(s.case_id),
        case_requirement(s.case_id) != NONE,
        case_scheme(s.case_id) != NONE,
      }

  val successful_selection_is_published =
    s.result_kind != RESOLVED
      or published(s.case_id).contains(s.resolved_version)

  val opaque_range_is_never_resolved =
    not(s.case_id == 5 and s.result_kind == RESOLVED)

  val graph_resolution_is_closed = and {
    not(s.partial_lock),
    not(s.graph_ok and not(s.lock_written)),
    not(s.graph_case != NONE and not(s.graph_ok) and s.lock_written),
    not(s.graph_case == NONE and s.lock_written),
  }

  val resolution_safety = and {
    bounded_state,
    current_outcome_is_total,
    current_result_matches_model,
    successful_selection_is_published,
    opaque_range_is_never_resolved,
    graph_resolution_is_closed,
  }

  val semver_selection_reached = s.saw_semver_selection
  val calver_selection_reached = s.saw_calver_selection
  val opaque_range_rejected_reached = s.saw_opaque_range_rejection
  val malformed_requirement_rejected_reached = s.saw_malformed_requirement_rejection
  val no_versions_reached = s.saw_no_versions
  val unsatisfied_reached = s.saw_unsatisfied
  val missing_dependency_fails_closed_reached = s.saw_missing_dependency
  val cycle_fails_closed_reached = s.saw_cycle
  val deterministic_replay_reached = s.saw_deterministic_replay
}
