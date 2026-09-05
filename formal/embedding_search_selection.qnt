// Finite model of the canonical embedding search row-selection boundary.
//
// A new content digest creates another row for an entity/model pair. The SQL
// selector must retain exactly the row with the greatest updated_at, breaking
// equal-timestamp ties by id, before applying the caller's visibility set. The
// finite rows below represent two entities and two models; they deliberately
// include duplicate keys and timestamp ties.
module embedding_search_selection {
  pure val NONE = 0
  pure val ROWS = Set(1, 2, 3, 4, 5, 6, 7, 8)
  pure val KEYS = Set(11, 12, 21)
  pure val ENTITIES = Set(1, 2)
  pure val MODELS = Set(1, 2)

  type State = {
    rows: Set[int],
    visible_entities: Set[int],
    selected_rows: Set[int],
    returned_rows: Set[int],
    saw_duplicate_content_row: bool,
    saw_timestamp_tie_break: bool,
    saw_second_model_partition: bool,
    saw_visibility_filter: bool,
    saw_idempotent_replay: bool,
  }

  var s: State

  // Row identity is distinct from content identity. Rows 2/3 and 7/8 are
  // equal-timestamp alternatives for the same key, so id is the deterministic
  // final tie-breaker exactly as in ORDER BY updated_at DESC, id DESC.
  pure def row_key(row) =
    if (row <= 3) 11
    else if (row <= 5) 12
    else 21

  pure def row_entity(row) = if (row <= 5) 1 else 2

  pure def row_model(row) =
    if (row <= 3 or row >= 6) 1 else 2

  pure def row_updated_at(row) =
    if (row == 1 or row == 4 or row == 6) 1
    else if (row == 2 or row == 3 or row == 7 or row == 8) 2
    else 3

  pure def latest_for_key(rows, key) =
    if (key == 11)
      if (rows.contains(3)) 3
      else if (rows.contains(2)) 2
      else if (rows.contains(1)) 1
      else NONE
    else if (key == 12)
      if (rows.contains(5)) 5
      else if (rows.contains(4)) 4
      else NONE
    else if (key == 21)
      if (rows.contains(8)) 8
      else if (rows.contains(7)) 7
      else if (rows.contains(6)) 6
      else NONE
    else NONE

  pure def include_if_present(row) = if (row == NONE) Set() else Set(row)

  pure def expected_selected(rows) =
    include_if_present(latest_for_key(rows, 11))
      .union(include_if_present(latest_for_key(rows, 12)))
      .union(include_if_present(latest_for_key(rows, 21)))

  pure def visible_row(row, visible_entities) =
    if (row == NONE or not(visible_entities.contains(row_entity(row)))) Set()
    else Set(row)

  pure def expected_returned(rows, visible_entities) =
    visible_row(latest_for_key(rows, 11), visible_entities)
      .union(visible_row(latest_for_key(rows, 12), visible_entities))
      .union(visible_row(latest_for_key(rows, 21), visible_entities))

  pure def duplicate_key_present(rows, row) =
    if (row == 1) rows.contains(2) or rows.contains(3)
    else if (row == 2 or row == 3) rows.contains(1) or rows.contains(2) and row == 3
    else if (row == 4) rows.contains(5)
    else if (row == 5) rows.contains(4)
    else if (row == 6) rows.contains(7) or rows.contains(8)
    else if (row == 7 or row == 8) rows.contains(6) or rows.contains(7) and row == 8
    else false

  pure def tie_break_present(rows) =
    and {
      or { rows.contains(2) and rows.contains(3), rows.contains(7) and rows.contains(8) },
      or { latest_for_key(rows, 11) == 3, latest_for_key(rows, 21) == 8 },
    }

  action init = {
    s' = {
      rows: Set(),
      visible_entities: ENTITIES,
      selected_rows: Set(),
      returned_rows: Set(),
      saw_duplicate_content_row: false,
      saw_timestamp_tie_break: false,
      saw_second_model_partition: false,
      saw_visibility_filter: false,
      saw_idempotent_replay: false,
    }
  }

  action insert(row) = all {
    ROWS.contains(row),
    not(s.rows.contains(row)),
    s' = {
      ...s,
      rows: s.rows.union(Set(row)),
      selected_rows: expected_selected(s.rows.union(Set(row))),
      returned_rows: expected_returned(s.rows.union(Set(row)), s.visible_entities),
      saw_duplicate_content_row:
        s.saw_duplicate_content_row or duplicate_key_present(s.rows, row),
      saw_timestamp_tie_break:
        s.saw_timestamp_tie_break or tie_break_present(s.rows.union(Set(row))),
      saw_second_model_partition:
        s.saw_second_model_partition or row == 4 or row == 5
          or s.rows.contains(4) or s.rows.contains(5),
    },
  }

  action hide(entity) = all {
    ENTITIES.contains(entity),
    s.visible_entities.contains(entity),
    s' = {
      ...s,
      visible_entities: s.visible_entities.exclude(Set(entity)),
      returned_rows: expected_returned(
        s.rows,
        s.visible_entities.exclude(Set(entity)),
      ),
      saw_visibility_filter:
        s.saw_visibility_filter or expected_returned(
          s.rows,
          s.visible_entities.exclude(Set(entity)),
        ).size() < s.selected_rows.size(),
    },
  }

  action show(entity) = all {
    ENTITIES.contains(entity),
    not(s.visible_entities.contains(entity)),
    s' = {
      ...s,
      visible_entities: s.visible_entities.union(Set(entity)),
      returned_rows: expected_returned(
        s.rows,
        s.visible_entities.union(Set(entity)),
      ),
    },
  }

  // A repeated upsert of the same content digest is a no-op at the selector
  // boundary; the unique key is (entity_type, entity_id, model, sha256).
  action replay_insert(row) = all {
    ROWS.contains(row),
    s.rows.contains(row),
    s' = { ...s, saw_idempotent_replay: true },
  }

  action idle = s' = s

  action step = {
    nondet row = ROWS.oneOf()
    nondet entity = ENTITIES.oneOf()
    any {
      insert(row),
      hide(entity),
      show(entity),
      replay_insert(row),
      idle,
    }
  }

  val rows_are_bounded = and {
    s.rows.subseteq(ROWS),
    s.selected_rows.subseteq(s.rows),
    s.returned_rows.subseteq(s.selected_rows),
    s.visible_entities.subseteq(ENTITIES),
  }

  val row_metadata_is_well_formed = and {
    ROWS.forall(row => KEYS.contains(row_key(row))),
    ROWS.forall(row => ENTITIES.contains(row_entity(row))),
    ROWS.forall(row => MODELS.contains(row_model(row))),
    ROWS.forall(row => row_updated_at(row) > 0),
  }

  val latest_row_is_selected_once_per_entity_model =
    s.selected_rows == expected_selected(s.rows)

  val visibility_is_applied_after_selection =
    s.returned_rows == expected_returned(s.rows, s.visible_entities)

  val selected_rows_are_model_partitioned = and {
    not(or {
      s.selected_rows.contains(1) and s.selected_rows.contains(2),
      s.selected_rows.contains(1) and s.selected_rows.contains(3),
      s.selected_rows.contains(2) and s.selected_rows.contains(3),
    }),
    not(s.selected_rows.contains(4) and s.selected_rows.contains(5)),
    not(or {
      s.selected_rows.contains(6) and s.selected_rows.contains(7),
      s.selected_rows.contains(6) and s.selected_rows.contains(8),
      s.selected_rows.contains(7) and s.selected_rows.contains(8),
    }),
  }

  val selection_safety = and {
    rows_are_bounded,
    row_metadata_is_well_formed,
    latest_row_is_selected_once_per_entity_model,
    visibility_is_applied_after_selection,
    selected_rows_are_model_partitioned,
  }

  val duplicate_content_row_reached = s.saw_duplicate_content_row
  val timestamp_tie_break_reached = s.saw_timestamp_tie_break
  val second_model_partition_reached = s.saw_second_model_partition
  val visibility_filter_reached = s.saw_visibility_filter
  val idempotent_replay_reached = s.saw_idempotent_replay
}
