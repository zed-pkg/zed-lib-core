// Finite resolver refinement model. Parsing and version precedence are abstracted
// into ranks/membership; the concrete spellings are exported for production replay.
// Selection scans candidates in order; the invariant independently quantifies over
// eligible candidates. No expected-answer table is used by the transition system.
module dependency_resolution {
  type Fixture = {
    name: str, scheme: str, mode: str, versions: List[str],
    ranks: List[int], matching: List[bool], stable: List[bool],
    requirement: str, latest: str, valid: bool, range: bool,
  }

  pure val CASES = 1.to(17)
  pure val EMPTY: Fixture = {
    name: "empty", scheme: "semver", mode: "resolve", versions: [],
    ranks: [], matching: [], stable: [], requirement: "^1", latest: "", valid: true, range: true,
  }

  pure def fixture(id) = {
    pure val semver: Fixture = {
      name: "stable-range", scheme: "semver", mode: "resolve",
      versions: ["1.0.0", "1.4.0", "1.5.0-rc.1", "2.0.0"],
      ranks: [10, 14, 15, 20], matching: [false, true, false, false],
      stable: [true, true, false, true], requirement: "^1.2", latest: "", valid: true, range: true,
    }
    pure val tie: Fixture = {
      ...EMPTY, name: "last-normalized-tie", versions: ["1.0.0", "v1.0.0"],
      ranks: [10, 10], matching: [true, true], stable: [true, true],
      requirement: "=1.0.0",
    }
    pure val calver: Fixture = {
      ...EMPTY, name: "calver-range", scheme: "calver",
      versions: ["2026.06.01", "2026.07.24", "2026.10.01"],
      ranks: [6, 7, 10], matching: [false, true, true],
      stable: [true, true, true], requirement: ">=2026.7",
    }
    pure val opaque: Fixture = {
      ...EMPTY, name: "opaque-exact", scheme: "opaque",
      versions: ["legacy-api", "nightly"], ranks: [1, 2],
      matching: [true, false], stable: [false, false], requirement: "legacy-api", range: false,
    }
    if (id == 1) semver
    else if (id == 2) tie
    else if (id == 3) calver
    else if (id == 4) opaque
    else if (id == 5) { ...opaque, name: "opaque-range", requirement: "^1.0", valid: false }
    else if (id == 6) { ...semver, name: "malformed-range", requirement: "^1.x.y", valid: false }
    else if (id == 7) { ...EMPTY, name: "no-versions" }
    else if (id == 8) {
      ...tie, name: "unsatisfied", requirement: "^9.0", matching: [false, false],
    }
    else if (id == 9) {
      ...semver, name: "latest-ignores-prerelease", mode: "latest", latest: "1.5.0-rc.1",
    }
    else if (id == 10) {
      ...opaque, name: "latest-withdrawn-opaque", mode: "latest", latest: "withdrawn",
    }
    else if (id == 11) { ...calver, name: "latest-calver-without-hint", mode: "latest" }
    else if (id == 12) { ...tie, name: "latest-last-normalized-tie", mode: "latest" }
    else if (id == 13) {
      ...EMPTY, name: "empty-before-malformed", requirement: "^1.x.y", valid: false,
    }
    else if (id == 14) { ...opaque, name: "latest-opaque-without-hint", mode: "latest" }
    else if (id == 15) { ...EMPTY, name: "latest-empty", mode: "latest" }
    else if (id == 16) {
      ...semver, name: "prerelease-range-is-not-stable", requirement: "=1.5.0-rc.1",
      matching: [false, false, true, false],
    }
    else {
      ...tie, name: "reversed-normalized-tie", versions: ["v1.0.0", "1.0.0"],
    }
  }

  pure def eligible(f, index) =
    // Range resolution is stable-only, including an explicit prerelease
    // comparator. Exact tags are verbatim and have no stability requirement.
    if (f.mode == "resolve") f.matching[index] and (not(f.range) or f.stable[index])
    else if (f.scheme == "opaque") f.versions[index] == f.latest
    else f.stable[index]

  type State = {
    case_id: int, input: Fixture, cursor: int, best: int, done: bool,
    kind: str, version: str, replaying: bool, previous_kind: str, previous_version: str,
    graph_case: int, graph_stage: int, staged_root: int, staged_child: int,
    locked_root: int, locked_child: int,
  }
  var s: State

  action init = s' = {
    case_id: 0, input: EMPTY, cursor: 0, best: -1, done: false,
    kind: "pending", version: "", replaying: false, previous_kind: "", previous_version: "",
    graph_case: 0, graph_stage: 0, staged_root: 0, staged_child: 0,
    locked_root: 0, locked_child: 0,
  }

  action begin(id, replay) = {
    pure val f = fixture(id)
    pure val no_versions = f.mode == "resolve" and f.versions.length() == 0
    pure val invalid = f.mode == "resolve" and not(no_versions) and not(f.valid)
    all {
      CASES.contains(id),
      s' = {
        ...s, case_id: id, input: f, cursor: 0, best: -1,
        done: no_versions or invalid, version: "", replaying: replay,
        kind: if (no_versions) "no_versions" else if (invalid) "invalid_requirement" else "pending",
        previous_kind: if (replay) s.kind else "",
        previous_version: if (replay) s.version else "",
      },
    }
  }

  action scan = all {
    s.case_id != 0,
    not(s.done),
    s.cursor < s.input.versions.length(),
    s' = {
      ...s, cursor: s.cursor + 1,
      best: if (not(eligible(s.input, s.cursor))) s.best
        else if (s.best == -1) s.cursor
        else if (s.input.ranks[s.cursor] >= s.input.ranks[s.best]) s.cursor
        else s.best,
    },
  }

  action finish = all {
    s.case_id != 0,
    not(s.done),
    s.cursor == s.input.versions.length(),
    s' = {
      ...s, done: true,
      kind: if (s.input.mode == "latest" or s.best != -1) "resolved" else "unsatisfied",
      version: if (s.best == -1) "" else s.input.versions[s.best],
    },
  }

  // Two-node graph abstraction from DEN-99: stage both nodes before publishing.
  // Case 2 has no transitive version; case 3 has a cycle. It is a model contract,
  // not a claim that the polyglot scalar resolver implements a graph solver.
  action begin_graph(id) = all {
    Set(1, 2, 3).contains(id),
    s' = {
      ...s, graph_case: id, graph_stage: 1, staged_root: 1, staged_child: 0,
      locked_root: 0, locked_child: 0,
    },
  }

  action visit_child = all {
    s.graph_stage == 1,
    s' = if (s.graph_case == 2) {
      ...s, graph_stage: 3, staged_root: 0, staged_child: 0,
    } else {
      ...s, graph_stage: 2, staged_child: 1,
    },
  }

  action finish_graph = all {
    s.graph_stage == 2,
    s' = if (s.graph_case == 3) {
      ...s, graph_stage: 3, staged_root: 0, staged_child: 0,
    } else {
      ...s, graph_stage: 4, locked_root: s.staged_root, locked_child: s.staged_child,
    },
  }

  action idle = s' = s

  action step = {
    nondet id = CASES.oneOf()
    nondet graph = Set(1, 2, 3).oneOf()
    any {
      all { s.case_id == 0 or s.done, begin(id, false) },
      all { s.done, not(s.replaying), begin(s.case_id, true) },
      scan, finish, begin_graph(graph), visit_child, finish_graph, idle,
    }
  }

  // Deterministic trace driver; it uses exactly the same begin/scan/finish
  // actions as step. Every fixture is replayed twice, then the trace stutters.
  action trace_step =
    if (s.case_id == 0) begin(1, false)
    else if (not(s.done)) {
      any { scan, finish }
    } else if (not(s.replaying)) begin(s.case_id, true)
    else if (s.case_id < 17) begin(s.case_id + 1, false)
    else idle

  val fixture_shape = and {
    s.input.versions.length() == s.input.ranks.length(),
    s.input.versions.length() == s.input.matching.length(),
    s.input.versions.length() == s.input.stable.length(),
    s.cursor >= 0 and s.cursor <= s.input.versions.length(),
    s.case_id == 0 or CASES.contains(s.case_id),
  }

  // Independent relational specification of the scanned prefix, including the
  // last-maximum spelling rule. Changing >= to > in scan violates this invariant.
  val maximum_prefix = {
    val visited = s.input.versions.indices().filter(i => i < s.cursor)
    val candidates = visited.filter(i => eligible(s.input, i))
    if (s.best == -1) candidates == Set()
    else and {
      candidates.contains(s.best),
      candidates.forall(i =>
        s.input.ranks[i] < s.input.ranks[s.best]
          or (s.input.ranks[i] == s.input.ranks[s.best] and i <= s.best)),
    }
  }

  val total_result = if (not(s.done)) {
    s.kind == "pending" and s.version == ""
  } else if (s.input.mode == "resolve" and s.input.versions.length() == 0) {
    s.kind == "no_versions" and s.version == ""
  } else if (s.input.mode == "resolve" and not(s.input.valid)) {
    s.kind == "invalid_requirement" and s.version == ""
  } else and {
    s.cursor == s.input.versions.length(),
    if (s.best == -1)
      s.version == "" and s.kind == (if (s.input.mode == "latest") "resolved" else "unsatisfied")
    else s.kind == "resolved" and s.version == s.input.versions[s.best],
  }

  val replay_agrees =
    not(s.done and s.replaying)
      or (s.kind == s.previous_kind and s.version == s.previous_version)

  val graph_resolution_is_closed = and {
    (s.locked_root == 0) == (s.locked_child == 0),
    if (s.graph_stage == 4)
      s.graph_case == 1 and s.locked_root == 1 and s.locked_child == 1
    else s.locked_root == 0 and s.locked_child == 0,
    if (s.graph_stage == 3) s.staged_root == 0 and s.staged_child == 0 else true,
  }

  val resolution_safety = and {
    fixture_shape, maximum_prefix, total_result, replay_agrees, graph_resolution_is_closed,
  }
  val semver_selection_reached = s.done and s.case_id == 1
  val calver_selection_reached = s.done and s.case_id == 3
  val opaque_range_rejected_reached = s.done and s.case_id == 5
  val malformed_requirement_rejected_reached = s.done and s.case_id == 6
  val no_versions_reached = s.done and s.case_id == 7
  val unsatisfied_reached = s.done and s.case_id == 8
  val missing_dependency_fails_closed_reached = s.graph_case == 2 and s.graph_stage == 3
  val cycle_fails_closed_reached = s.graph_case == 3 and s.graph_stage == 3
  val deterministic_replay_reached = s.done and s.replaying and s.case_id == 2
  val normalized_tie_reached = s.done and s.case_id == 17
  val prerelease_range_rejected_reached = s.done and s.case_id == 16 and s.kind == "unsatisfied"
  val stale_latest_reached = s.done and s.case_id == 9
  val absent_latest_reached = s.done and s.case_id == 14
}
