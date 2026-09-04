use anyhow::{Context, Result, ensure};
use serde_json::Value;
use zed_lock::{WaiterCompletionDisposition, settle_waiter_completion};

const CORPUS: &str = include_str!("../protocol/formal-waiter-lifecycle.json");

fn corpus() -> Result<Value> {
    serde_json::from_str(CORPUS).context("parse formal waiter lifecycle corpus")
}

#[test]
fn production_completion_decision_refines_every_formal_case() -> Result<()> {
    let document = corpus()?;
    let cases = document["completion_cases"]
        .as_array()
        .context("completion_cases must be an array")?;

    for case in cases {
        let name = case["name"]
            .as_str()
            .context("case name must be a string")?;
        let receiver_alive = case["receiver_alive"]
            .as_bool()
            .context("receiver_alive must be a boolean")?;
        let expected = case["expected_disposition"]
            .as_str()
            .context("expected_disposition must be a string")?;
        let actual = match settle_waiter_completion(receiver_alive) {
            WaiterCompletionDisposition::PublishResult => "publish_result",
            WaiterCompletionDisposition::ReleaseDetached => "release_detached",
        };
        ensure!(
            actual == expected,
            "{name}: expected {expected}, got {actual}"
        );
    }

    Ok(())
}

#[test]
fn terminal_reasons_are_disjoint_in_the_shared_corpus() -> Result<()> {
    let document = corpus()?;
    let cases = document["terminal_event_cases"]
        .as_array()
        .context("terminal_event_cases must be an array")?;

    for case in cases {
        let name = case["name"]
            .as_str()
            .context("case name must be a string")?;
        let expected = case["expected_events"]
            .as_array()
            .context("expected_events must be an array")?;
        let forbidden = case["forbidden_events"]
            .as_array()
            .context("forbidden_events must be an array")?;
        ensure!(
            expected.len() == 1,
            "{name}: exactly one terminal event required"
        );
        ensure!(
            !forbidden.contains(&expected[0]),
            "{name}: one event cannot be both expected and forbidden"
        );
    }

    Ok(())
}

#[test]
fn lockset_release_order_is_the_reverse_of_acquisition() -> Result<()> {
    let document = corpus()?;
    let cases = document["lockset_unwind_cases"]
        .as_array()
        .context("lockset_unwind_cases must be an array")?;

    for case in cases {
        let name = case["name"]
            .as_str()
            .context("case name must be a string")?;
        let acquired = case["acquired_guard_ids"]
            .as_array()
            .context("acquired_guard_ids must be an array")?;
        let expected = case["expected_release_order"]
            .as_array()
            .context("expected_release_order must be an array")?;
        let actual = acquired.iter().rev().collect::<Vec<_>>();
        let expected_refs = expected.iter().collect::<Vec<_>>();
        ensure!(
            actual == expected_refs,
            "{name}: release order is not reverse"
        );
    }

    Ok(())
}
