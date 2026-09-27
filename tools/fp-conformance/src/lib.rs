#![allow(clippy::needless_return)]

pub mod model;
pub mod report;
pub mod rules;
pub mod scanner;

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::model::Language;
    use crate::report::{parse_count_object, regressions};
    use crate::scanner::analyse_text;

    #[test]
    fn rust_golden_rules_match_legacy_semantics() {
        let source = r#"
let mut value = 1;
static mut GLOBAL: usize = 0;
pub static LOCK: Mutex<usize> = Mutex::new(0);
let x = maybe.unwrap();
_ => 1,
fn bad() -> Box<dyn std::error::Error> { todo!() }
let cell: RefCell<u8> = RefCell::new(0);
println!("x");
fn change(&mut self) {
unsafe { do_it(); }
"#;
        let result = analyse_text("src/domain.rs", Language::Rust, source);
        let counts = result.counts();
        assert_eq!(counts.get("RS001"), Some(&1));
        assert_eq!(counts.get("RS002"), Some(&2));
        assert_eq!(counts.get("RS003"), Some(&2));
        assert_eq!(counts.get("RS004"), Some(&1));
        assert_eq!(counts.get("RS005"), Some(&1));
        assert_eq!(counts.get("RS006"), Some(&1));
        assert_eq!(counts.get("RS007"), Some(&1));
        assert_eq!(counts.get("RS008"), Some(&1));
        assert_eq!(counts.get("RS009"), Some(&1));
    }

    #[test]
    fn typescript_golden_rules_match_legacy_semantics() {
        let source = r#"
var old = 1;
  let local = 2;
export let global = 3;
items.push(local);
const escape: any = global;
throw failure;
import React from "react";
console.log(global);
const now = Date.now();
value!.run();
"#;
        let counts = analyse_text("src/domain.ts", Language::TypeScript, source).counts();
        for code in [
            "TS001", "TS002", "TS004", "TS005", "TS006", "TS007", "TS008", "TS009", "TS010",
        ] {
            assert_eq!(counts.get(code), Some(&1), "{code}");
        }
        assert_eq!(counts.get("TS003"), Some(&2));
    }

    #[test]
    fn dart_golden_rules_match_legacy_semantics() {
        let source = r#"
var local = 1;
String globalName = "x";
  String mutableField;
late String deferred;
throw failure;
print(local);
value!.run();
items.add(local);
default:
"#;
        let counts = analyse_text("lib/domain.dart", Language::Dart, source).counts();
        for code in [
            "DA001", "DA003", "DA004", "DA005", "DA006", "DA007", "DA008", "DA009",
        ] {
            assert_eq!(counts.get(code), Some(&1), "{code}");
        }
        assert_eq!(counts.get("DA002"), Some(&2));
    }

    #[test]
    fn exact_base_comparison_is_per_rule_and_zero_fills_missing_rules() {
        let base = BTreeMap::from([("RS001".to_owned(), 4_usize)]);
        let head = BTreeMap::from([("RS001".to_owned(), 4_usize), ("RS002".to_owned(), 1_usize)]);
        assert_eq!(regressions(&base, &head), vec![("RS002".to_owned(), 0, 1)]);
    }

    #[test]
    fn count_json_parser_reads_scanner_and_budget_objects() {
        let scanner = r#"{"counts":{"RS001":111,"RS003":91}}"#;
        let budget = r#"{"budget":{"RS001":57,"RS003":56}}"#;
        assert_eq!(
            parse_count_object(scanner, "counts").and_then(|counts| counts.get("RS001").copied()),
            Some(111)
        );
        assert_eq!(
            parse_count_object(budget, "budget").and_then(|counts| counts.get("RS003").copied()),
            Some(56)
        );
    }
}
