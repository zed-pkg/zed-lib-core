// Finite model of zed-lock's event-driven waiter and ordered lock-set lifecycle.
//
// The operating system's descriptor lock is the only ownership authority. A
// Unix timeout or cancellation abandons delivery but cannot cancel a native
// syscall already blocked in the kernel. If that detached request is granted,
// the worker must immediately release it instead of publishing a guard.
module waiter_lifecycle {
  pure val NONE = 0
  pure val WAITER_IDS = Set(1, 2, 3)
  pure val MAX_WAITERS = 2

  pure val LOCKSET_IDLE = 0
  pure val LOCKSET_LOW_HELD = 1
  pure val LOCKSET_COMPLETE = 2
  pure val LOCKSET_UNWOUND = 3
  pure val LOCKSET_RELEASED = 4

  type State = {
    started: Set[int],
    active_workers: Set[int],
    receiver_alive: Set[int],
    result_ready: Set[int],
    completed_workers: Set[int],
    abandoned: Set[int],
    timed_out: Set[int],
    cancelled: Set[int],
    detached_released: Set[int],
    delivered: Set[int],
    released: Set[int],
    kernel_owner: int,
    published_owner: int,
    last_released_owner: int,
    lockset_status: int,
    low_lock_held: bool,
    high_lock_held: bool,
    saw_cancel_before_grant: bool,
    saw_timeout_before_grant: bool,
    saw_detached_grant_release: bool,
    saw_successful_transfer: bool,
    saw_waiter_cap_rejection: bool,
    saw_same_process_rejection: bool,
    saw_partial_lockset_unwind: bool,
  }

  var s: State

  action init = {
    s' = {
      started: Set(),
      active_workers: Set(),
      receiver_alive: Set(),
      result_ready: Set(),
      completed_workers: Set(),
      abandoned: Set(),
      timed_out: Set(),
      cancelled: Set(),
      detached_released: Set(),
      delivered: Set(),
      released: Set(),
      kernel_owner: NONE,
      published_owner: NONE,
      last_released_owner: NONE,
      lockset_status: LOCKSET_IDLE,
      low_lock_held: false,
      high_lock_held: false,
      saw_cancel_before_grant: false,
      saw_timeout_before_grant: false,
      saw_detached_grant_release: false,
      saw_successful_transfer: false,
      saw_waiter_cap_rejection: false,
      saw_same_process_rejection: false,
      saw_partial_lockset_unwind: false,
    }
  }

  // SameProcessPolicy::Queue is explicit here. The default rejection path is
  // represented separately by reject_same_process below.
  action start_queued(waiter) = all {
    WAITER_IDS.contains(waiter),
    not(s.started.contains(waiter)),
    s.active_workers.size() < MAX_WAITERS,
    s' = {
      ...s,
      started: s.started.union(Set(waiter)),
      active_workers: s.active_workers.union(Set(waiter)),
      receiver_alive: s.receiver_alive.union(Set(waiter)),
    },
  }

  action reject_at_waiter_cap(waiter) = all {
    WAITER_IDS.contains(waiter),
    not(s.started.contains(waiter)),
    s.active_workers.size() == MAX_WAITERS,
    s' = { ...s, saw_waiter_cap_rejection: true },
  }

  action reject_same_process(waiter) = all {
    WAITER_IDS.contains(waiter),
    not(s.started.contains(waiter)),
    or {
      s.active_workers.size() > 0,
      s.kernel_owner != NONE,
    },
    s' = { ...s, saw_same_process_rejection: true },
  }

  action cancel_before_grant(waiter) = all {
    s.active_workers.contains(waiter),
    s.receiver_alive.contains(waiter),
    not(s.result_ready.contains(waiter)),
    s' = {
      ...s,
      receiver_alive: s.receiver_alive.exclude(Set(waiter)),
      abandoned: s.abandoned.union(Set(waiter)),
      cancelled: s.cancelled.union(Set(waiter)),
      saw_cancel_before_grant: true,
    },
  }

  action timeout_before_grant(waiter) = all {
    s.active_workers.contains(waiter),
    s.receiver_alive.contains(waiter),
    not(s.result_ready.contains(waiter)),
    s' = {
      ...s,
      receiver_alive: s.receiver_alive.exclude(Set(waiter)),
      abandoned: s.abandoned.union(Set(waiter)),
      timed_out: s.timed_out.union(Set(waiter)),
      saw_timeout_before_grant: true,
    },
  }

  // A native grant completes the worker and returns its waiter permit. A live
  // receiver gets a pending result that still owns the descriptor lock. An
  // abandoned receiver causes the newly acquired guard to be dropped at once.
  action native_grant(waiter) = all {
    s.active_workers.contains(waiter),
    s.kernel_owner == NONE,
    if (s.receiver_alive.contains(waiter))
      s' = {
        ...s,
        active_workers: s.active_workers.exclude(Set(waiter)),
        completed_workers: s.completed_workers.union(Set(waiter)),
        result_ready: s.result_ready.union(Set(waiter)),
        kernel_owner: waiter,
      }
    else
      s' = {
        ...s,
        active_workers: s.active_workers.exclude(Set(waiter)),
        completed_workers: s.completed_workers.union(Set(waiter)),
        detached_released: s.detached_released.union(Set(waiter)),
        saw_detached_grant_release: true,
      },
  }

  action deliver_guard(waiter) = all {
    s.result_ready.contains(waiter),
    s.receiver_alive.contains(waiter),
    s.kernel_owner == waiter,
    s.published_owner == NONE,
    s' = {
      ...s,
      result_ready: s.result_ready.exclude(Set(waiter)),
      receiver_alive: s.receiver_alive.exclude(Set(waiter)),
      delivered: s.delivered.union(Set(waiter)),
      published_owner: waiter,
      saw_successful_transfer:
        s.saw_successful_transfer or and {
          s.last_released_owner != NONE,
          s.last_released_owner != waiter,
        },
    },
  }

  // Dropping a waiter after the worker stored its result drops that result and
  // therefore the guard. This is cancellation, not timeout: acquire_timeout
  // records timeout explicitly and suppresses the generic cancellation event.
  action cancel_ready_result(waiter) = all {
    s.result_ready.contains(waiter),
    s.receiver_alive.contains(waiter),
    s.kernel_owner == waiter,
    s' = {
      ...s,
      result_ready: s.result_ready.exclude(Set(waiter)),
      receiver_alive: s.receiver_alive.exclude(Set(waiter)),
      abandoned: s.abandoned.union(Set(waiter)),
      cancelled: s.cancelled.union(Set(waiter)),
      detached_released: s.detached_released.union(Set(waiter)),
      kernel_owner: NONE,
      saw_detached_grant_release: true,
    },
  }

  action release_guard(waiter) = all {
    s.published_owner == waiter,
    s.kernel_owner == waiter,
    not(s.released.contains(waiter)),
    s' = {
      ...s,
      released: s.released.union(Set(waiter)),
      kernel_owner: NONE,
      published_owner: NONE,
      last_released_owner: waiter,
    },
  }

  // The ordered lock-set abstraction always acquires the lower class/path
  // first. Failure of the second acquisition must explicitly unwind the first.
  action start_lockset = all {
    s.lockset_status == LOCKSET_IDLE,
    s' = {
      ...s,
      lockset_status: LOCKSET_LOW_HELD,
      low_lock_held: true,
    },
  }

  action finish_lockset = all {
    s.lockset_status == LOCKSET_LOW_HELD,
    s.low_lock_held,
    not(s.high_lock_held),
    s' = {
      ...s,
      lockset_status: LOCKSET_COMPLETE,
      high_lock_held: true,
    },
  }

  action fail_lockset_and_unwind = all {
    s.lockset_status == LOCKSET_LOW_HELD,
    s.low_lock_held,
    not(s.high_lock_held),
    s' = {
      ...s,
      lockset_status: LOCKSET_UNWOUND,
      low_lock_held: false,
      saw_partial_lockset_unwind: true,
    },
  }

  action release_lockset = all {
    s.lockset_status == LOCKSET_COMPLETE,
    s.low_lock_held,
    s.high_lock_held,
    s' = {
      ...s,
      lockset_status: LOCKSET_RELEASED,
      low_lock_held: false,
      high_lock_held: false,
    },
  }

  action idle = s' = s

  action step = {
    nondet waiter = WAITER_IDS.oneOf()
    any {
      start_queued(waiter),
      reject_at_waiter_cap(waiter),
      reject_same_process(waiter),
      cancel_before_grant(waiter),
      timeout_before_grant(waiter),
      native_grant(waiter),
      deliver_guard(waiter),
      cancel_ready_result(waiter),
      release_guard(waiter),
      start_lockset,
      finish_lockset,
      fail_lockset_and_unwind,
      release_lockset,
      idle,
    }
  }

  val waiter_sets_are_well_formed = and {
    s.started.subseteq(WAITER_IDS),
    s.active_workers.subseteq(s.started),
    s.receiver_alive.subseteq(s.started),
    s.result_ready.subseteq(s.completed_workers),
    s.completed_workers.subseteq(s.started),
    s.abandoned.subseteq(s.started),
    s.timed_out.subseteq(s.abandoned),
    s.cancelled.subseteq(s.abandoned),
    s.detached_released.subseteq(s.completed_workers),
    s.delivered.subseteq(s.completed_workers),
    s.released.subseteq(s.delivered),
    s.active_workers.intersect(s.completed_workers).size() == 0,
    s.result_ready.intersect(s.abandoned).size() == 0,
    s.delivered.intersect(s.abandoned).size() == 0,
    s.timed_out.intersect(s.cancelled).size() == 0,
  }

  val waiter_cap_is_bounded = s.active_workers.size() <= MAX_WAITERS

  val ownership_is_exclusive = and {
    or { s.kernel_owner == NONE, WAITER_IDS.contains(s.kernel_owner) },
    or { s.published_owner == NONE, WAITER_IDS.contains(s.published_owner) },
    if (s.published_owner == NONE)
      s.result_ready.size() == if (s.kernel_owner == NONE) 0 else 1
    else and {
      s.kernel_owner == s.published_owner,
      s.result_ready.size() == 0,
    },
    if (s.result_ready.size() == 1)
      s.result_ready.contains(s.kernel_owner)
    else true,
  }

  val abandoned_delivery_is_impossible = and {
    s.delivered.intersect(s.abandoned).size() == 0,
    s.detached_released.intersect(s.abandoned) == s.abandoned.intersect(s.completed_workers),
  }

  val terminal_reason_is_unique = s.timed_out.intersect(s.cancelled).size() == 0

  val ordered_lockset_is_safe = and {
    not(s.high_lock_held) or s.low_lock_held,
    if (s.lockset_status == LOCKSET_IDLE) and {
      not(s.low_lock_held),
      not(s.high_lock_held),
    } else true,
    if (s.lockset_status == LOCKSET_LOW_HELD) and {
      s.low_lock_held,
      not(s.high_lock_held),
    } else true,
    if (s.lockset_status == LOCKSET_COMPLETE) and {
      s.low_lock_held,
      s.high_lock_held,
    } else true,
    if (or {
      s.lockset_status == LOCKSET_UNWOUND,
      s.lockset_status == LOCKSET_RELEASED,
    }) and {
      not(s.low_lock_held),
      not(s.high_lock_held),
    } else true,
  }

  val waiter_lifecycle_safety = and {
    waiter_sets_are_well_formed,
    waiter_cap_is_bounded,
    ownership_is_exclusive,
    abandoned_delivery_is_impossible,
    terminal_reason_is_unique,
    ordered_lockset_is_safe,
  }

  val cancel_before_grant_reached = s.saw_cancel_before_grant
  val timeout_before_grant_reached = s.saw_timeout_before_grant
  val detached_grant_release_reached = s.saw_detached_grant_release
  val successful_transfer_reached = s.saw_successful_transfer
  val waiter_cap_rejection_reached = s.saw_waiter_cap_rejection
  val same_process_rejection_reached = s.saw_same_process_rejection
  val partial_lockset_unwind_reached = s.saw_partial_lockset_unwind
}
