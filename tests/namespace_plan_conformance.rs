use std::fs;
use std::path::PathBuf;

use serde::Deserialize;
use zed_interfaces::{
    RegistryNamespaceAction, RegistryNamespaceAutomation, RegistryNamespaceDisposition,
    RegistryNamespaceProof, RegistryNamespaceProvider, RegistryNamespaceRequest,
};
use zed_lib::plan_registry_namespaces;

#[derive(Debug, Deserialize)]
struct Corpus {
    schema: String,
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct Case {
    name: String,
    request: RegistryNamespaceRequest,
    expected: Vec<ExpectedEntry>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct ExpectedEntry {
    provider: RegistryNamespaceProvider,
    coordinate: Option<String>,
    package_prefix: Option<String>,
    automation: RegistryNamespaceAutomation,
    disposition: RegistryNamespaceDisposition,
    proofs: Vec<RegistryNamespaceProof>,
    step_actions: Vec<RegistryNamespaceAction>,
}

#[test]
fn registry_namespace_planner_matches_shared_corpus() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("conformance/cases/registry-namespace-plans.json");
    let corpus: Corpus = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        corpus.schema,
        "zed.registry-namespace-planner-cases/v1"
    );
    assert!(!corpus.cases.is_empty());

    for case in corpus.cases {
        let plan = plan_registry_namespaces(case.request).unwrap();
        let observed = plan
            .entries
            .iter()
            .map(|entry| ExpectedEntry {
                provider: entry.provider,
                coordinate: entry.coordinate.clone(),
                package_prefix: entry.package_prefix.clone(),
                automation: entry.automation,
                disposition: entry.disposition,
                proofs: entry.proofs.clone(),
                step_actions: entry.steps.iter().map(|step| step.action).collect(),
            })
            .collect::<Vec<_>>();
        assert_eq!(observed, case.expected, "case `{}`", case.name);
        assert_eq!(
            plan.request.providers,
            observed.iter().map(|entry| entry.provider).collect::<Vec<_>>(),
            "case `{}` provider order",
            case.name
        );
    }
}
