//! Provider-aware planning for coordinated package-registry and forge names.
//!
//! The output is deliberately pre-mutation. It distinguishes literal scopes,
//! proof-gated coordinates, manual forge entities, and registries with only
//! global package names. Availability checks and claim execution are separate
//! adapters and must produce receipts before external ownership is asserted.

use zed_interfaces::{
    REGISTRY_NAMESPACE_PLAN_SCHEMA_V1, RegistryNamespaceAction, RegistryNamespaceAutomation,
    RegistryNamespaceDisposition, RegistryNamespaceEntry, RegistryNamespaceError,
    RegistryNamespaceModel, RegistryNamespacePlan, RegistryNamespaceProof,
    RegistryNamespaceProvider, RegistryNamespaceRequest, RegistryNamespaceStep,
};

const PLAN_WARNING: &str =
    "This plan is pre-mutation intent and is not external namespace ownership evidence.";
const RACE_WARNING: &str =
    "Provider availability can change between planning, manual proof, and claim execution.";

/// Build one deterministic provider-aware namespace plan.
///
/// The request is validated before any coordinate is derived. The resulting
/// plan contains exactly one entry for every requested provider, sorted by the
/// shared contract's provider order.
pub fn plan_registry_namespaces(
    request: RegistryNamespaceRequest,
) -> Result<RegistryNamespacePlan, RegistryNamespaceError> {
    request.validate()?;
    let request = request.normalized();
    let entries = request
        .providers
        .iter()
        .copied()
        .map(|provider| plan_provider(&request, provider))
        .collect::<Vec<_>>();
    let plan = RegistryNamespacePlan {
        schema: REGISTRY_NAMESPACE_PLAN_SCHEMA_V1.to_owned(),
        request,
        entries,
        warnings: vec![PLAN_WARNING.to_owned(), RACE_WARNING.to_owned()],
    };
    plan.validate()?;
    Ok(plan.normalized())
}

fn plan_provider(
    request: &RegistryNamespaceRequest,
    provider: RegistryNamespaceProvider,
) -> RegistryNamespaceEntry {
    match provider {
        RegistryNamespaceProvider::Npm => npm(request),
        RegistryNamespaceProvider::MavenCentral => maven_central(request),
        RegistryNamespaceProvider::CratesIo => crates_io(request),
        RegistryNamespaceProvider::PubDev => pub_dev(request),
        RegistryNamespaceProvider::GitHub => forge(
            provider,
            RegistryNamespaceModel::ForgeOrganization,
            RegistryNamespaceAction::CreateOrganization,
            "Create the GitHub organization through the account-owned organization flow.",
            request,
        ),
        RegistryNamespaceProvider::GitLabCom => forge(
            provider,
            RegistryNamespaceModel::ForgeGroup,
            RegistryNamespaceAction::CreateGroup,
            "Create the GitLab.com top-level group through the account-owned group flow.",
            request,
        ),
        RegistryNamespaceProvider::BitbucketCloud => forge(
            provider,
            RegistryNamespaceModel::ForgeWorkspace,
            RegistryNamespaceAction::CreateWorkspace,
            "Create the Bitbucket Cloud workspace through Atlassian Administration.",
            request,
        ),
    }
}

fn npm(request: &RegistryNamespaceRequest) -> RegistryNamespaceEntry {
    let coordinate = format!("@{}", request.brand);
    RegistryNamespaceEntry {
        provider: RegistryNamespaceProvider::Npm,
        model: RegistryNamespaceModel::LiteralOrganizationScope,
        coordinate: Some(coordinate.clone()),
        package_prefix: None,
        automation: RegistryNamespaceAutomation::ManualWebFlow,
        disposition: RegistryNamespaceDisposition::ManualActionRequired,
        proofs: vec![RegistryNamespaceProof::RegistryAccountControl],
        steps: vec![
            step(
                RegistryNamespaceAction::CheckAvailability,
                format!("Check whether npm organization scope `{coordinate}` is available."),
                false,
                None,
            ),
            step(
                RegistryNamespaceAction::CreateOrganization,
                format!(
                    "Create npm organization `{}` so the matching `{coordinate}` scope is owned by the organization.",
                    request.brand
                ),
                true,
                Some("Control an npm account authorized to create an organization."),
            ),
            step(
                RegistryNamespaceAction::RecordOwnershipEvidence,
                format!("Re-read npm organization `{}` and record non-secret ownership evidence.", request.brand),
                false,
                Some("The npm organization exists and the acting account is an owner."),
            ),
        ],
        warnings: vec![
            "Unscoped npm package names are global and are not protected by this organization claim."
                .to_owned(),
        ],
    }
}

fn maven_central(request: &RegistryNamespaceRequest) -> RegistryNamespaceEntry {
    match (&request.domain, &request.github_owner) {
        (Some(domain), _) => {
            let coordinate = reverse_domain(domain);
            RegistryNamespaceEntry {
                provider: RegistryNamespaceProvider::MavenCentral,
                model: RegistryNamespaceModel::VerifiedGroupIdPrefix,
                coordinate: Some(coordinate.clone()),
                package_prefix: None,
                automation: RegistryNamespaceAutomation::ManualWebFlow,
                disposition: RegistryNamespaceDisposition::ManualActionRequired,
                proofs: vec![
                    RegistryNamespaceProof::RegistryAccountControl,
                    RegistryNamespaceProof::DomainControl,
                ],
                steps: vec![
                    step(
                        RegistryNamespaceAction::CheckAvailability,
                        format!("Check whether Maven Central namespace `{coordinate}` is already registered."),
                        false,
                        None,
                    ),
                    step(
                        RegistryNamespaceAction::RegisterNamespace,
                        format!("Register Maven Central namespace `{coordinate}` in Central Portal."),
                        true,
                        Some("Control a Central Portal publishing account."),
                    ),
                    step(
                        RegistryNamespaceAction::VerifyDomain,
                        format!("Complete the provider challenge proving control of `{domain}`."),
                        true,
                        Some("Control DNS or another provider-approved proof channel for the domain."),
                    ),
                    step(
                        RegistryNamespaceAction::RecordOwnershipEvidence,
                        format!("Re-read verified Maven namespace `{coordinate}` and record non-secret evidence."),
                        false,
                        Some("Central Portal reports the namespace as verified."),
                    ),
                ],
                warnings: vec![
                    "A derived reverse-DNS groupId is only a candidate until Maven Central accepts the proof."
                        .to_owned(),
                ],
            }
        }
        (None, Some(owner)) => {
            let coordinate = format!("io.github.{owner}");
            RegistryNamespaceEntry {
                provider: RegistryNamespaceProvider::MavenCentral,
                model: RegistryNamespaceModel::VerifiedGroupIdPrefix,
                coordinate: Some(coordinate.clone()),
                package_prefix: None,
                automation: RegistryNamespaceAutomation::ManualWebFlow,
                disposition: RegistryNamespaceDisposition::ManualActionRequired,
                proofs: vec![
                    RegistryNamespaceProof::RegistryAccountControl,
                    RegistryNamespaceProof::GitHubAccountControl,
                ],
                steps: vec![
                    step(
                        RegistryNamespaceAction::CheckAvailability,
                        format!("Check whether Maven Central namespace `{coordinate}` is already registered."),
                        false,
                        None,
                    ),
                    step(
                        RegistryNamespaceAction::RegisterNamespace,
                        format!("Register Maven Central namespace `{coordinate}` in Central Portal."),
                        true,
                        Some("Control a Central Portal publishing account."),
                    ),
                    step(
                        RegistryNamespaceAction::RecordOwnershipEvidence,
                        format!("Complete GitHub-owner proof for `{owner}` and record the verified Central namespace."),
                        true,
                        Some("Control the explicitly named GitHub owner; ambient Git credentials are not proof."),
                    ),
                ],
                warnings: vec![
                    "The `io.github` coordinate is an explicit fallback, not a substitute for a controlled product domain."
                        .to_owned(),
                ],
            }
        }
        (None, None) => RegistryNamespaceEntry {
            provider: RegistryNamespaceProvider::MavenCentral,
            model: RegistryNamespaceModel::VerifiedGroupIdPrefix,
            coordinate: None,
            package_prefix: None,
            automation: RegistryNamespaceAutomation::ManualWebFlow,
            disposition: RegistryNamespaceDisposition::MissingPrerequisite,
            proofs: vec![
                RegistryNamespaceProof::DomainControl,
                RegistryNamespaceProof::GitHubAccountControl,
            ],
            steps: vec![step(
                RegistryNamespaceAction::RegisterNamespace,
                "Supply a controlled domain or an explicit GitHub owner before deriving a Maven namespace.",
                false,
                Some("A canonical domain is preferred; an explicit GitHub owner enables the `io.github` fallback."),
            )],
            warnings: vec![
                "No Maven coordinate was derived because neither domain nor explicit GitHub owner was supplied."
                    .to_owned(),
            ],
        },
    }
}

fn crates_io(request: &RegistryNamespaceRequest) -> RegistryNamespaceEntry {
    let prefix = format!("{}-", request.brand);
    RegistryNamespaceEntry {
        provider: RegistryNamespaceProvider::CratesIo,
        model: RegistryNamespaceModel::GlobalPackageNames,
        coordinate: None,
        package_prefix: Some(prefix.clone()),
        automation: RegistryNamespaceAutomation::NotReservable,
        disposition: RegistryNamespaceDisposition::NotReservable,
        proofs: vec![RegistryNamespaceProof::ExistingPackageOwnership],
        steps: vec![
            step(
                RegistryNamespaceAction::CheckAvailability,
                format!("Check every intended crates.io crate name using advisory prefix `{prefix}`."),
                false,
                None,
            ),
            step(
                RegistryNamespaceAction::PublishFirstPackage,
                "Publish each genuine crate to acquire that individual global crate name.",
                false,
                Some("The crate is release-ready and complies with crates.io publication policy."),
            ),
            step(
                RegistryNamespaceAction::AddOwnerTeam,
                "Add the intended GitHub users or team as crate owners after publication.",
                false,
                Some("At least one acting account already owns the published crate."),
            ),
            step(
                RegistryNamespaceAction::RecordOwnershipEvidence,
                "Record non-secret ownership evidence for each individual crate name.",
                false,
                Some("The crate exists and the expected owners are visible through crates.io."),
            ),
        ],
        warnings: vec![
            format!("`{prefix}` is a naming convention only; crates.io does not reserve organization prefixes."),
            "Do not publish empty placeholder crates solely to squat on names.".to_owned(),
        ],
    }
}

fn pub_dev(request: &RegistryNamespaceRequest) -> RegistryNamespaceEntry {
    match &request.domain {
        Some(domain) => RegistryNamespaceEntry {
            provider: RegistryNamespaceProvider::PubDev,
            model: RegistryNamespaceModel::VerifiedPublisherDomain,
            coordinate: Some(domain.clone()),
            package_prefix: None,
            automation: RegistryNamespaceAutomation::ManualWebFlow,
            disposition: RegistryNamespaceDisposition::ManualActionRequired,
            proofs: vec![
                RegistryNamespaceProof::RegistryAccountControl,
                RegistryNamespaceProof::DomainControl,
            ],
            steps: vec![
                step(
                    RegistryNamespaceAction::VerifyDomain,
                    format!("Prove control of `{domain}` through the pub.dev publisher flow."),
                    true,
                    Some("Control the domain verification channel and a pub.dev-linked Google account."),
                ),
                step(
                    RegistryNamespaceAction::CreatePublisher,
                    format!("Create verified pub.dev publisher `{domain}`."),
                    true,
                    Some("pub.dev accepts the domain-control proof."),
                ),
                step(
                    RegistryNamespaceAction::RecordOwnershipEvidence,
                    format!("Re-read publisher `{domain}` and record non-secret verification evidence."),
                    false,
                    Some("The expected account is an administrator of the verified publisher."),
                ),
            ],
            warnings: vec![
                "pub.dev package names remain global even when a package is associated with a verified publisher."
                    .to_owned(),
            ],
        },
        None => RegistryNamespaceEntry {
            provider: RegistryNamespaceProvider::PubDev,
            model: RegistryNamespaceModel::VerifiedPublisherDomain,
            coordinate: None,
            package_prefix: None,
            automation: RegistryNamespaceAutomation::ManualWebFlow,
            disposition: RegistryNamespaceDisposition::MissingPrerequisite,
            proofs: vec![RegistryNamespaceProof::DomainControl],
            steps: vec![step(
                RegistryNamespaceAction::VerifyDomain,
                "Supply and prove control of a canonical domain before creating a pub.dev publisher.",
                true,
                Some("A verified publisher is domain-derived; a brand slug alone is insufficient."),
            )],
            warnings: vec![
                "No pub.dev publisher coordinate was derived because no domain was supplied."
                    .to_owned(),
            ],
        },
    }
}

fn forge(
    provider: RegistryNamespaceProvider,
    model: RegistryNamespaceModel,
    action: RegistryNamespaceAction,
    create_summary: &str,
    request: &RegistryNamespaceRequest,
) -> RegistryNamespaceEntry {
    let coordinate = request.brand.clone();
    RegistryNamespaceEntry {
        provider,
        model,
        coordinate: Some(coordinate.clone()),
        package_prefix: None,
        automation: RegistryNamespaceAutomation::ManualWebFlow,
        disposition: RegistryNamespaceDisposition::ManualActionRequired,
        proofs: vec![RegistryNamespaceProof::ForgeAdministrator],
        steps: vec![
            step(
                RegistryNamespaceAction::CheckAvailability,
                format!("Check whether provider coordinate `{coordinate}` is currently available."),
                false,
                None,
            ),
            step(
                action,
                create_summary,
                true,
                Some("Use an account authorized to create and administer the provider entity."),
            ),
            step(
                RegistryNamespaceAction::RecordOwnershipEvidence,
                format!("Re-read `{coordinate}` and record non-secret administrator evidence."),
                false,
                Some("The entity exists and the expected account has administrator authority."),
            ),
        ],
        warnings: vec![
            "A read-only availability result does not reserve the coordinate and may race another claimant."
                .to_owned(),
        ],
    }
}

fn step(
    action: RegistryNamespaceAction,
    summary: impl Into<String>,
    manual: bool,
    prerequisite: Option<&str>,
) -> RegistryNamespaceStep {
    RegistryNamespaceStep {
        action,
        summary: summary.into(),
        manual,
        prerequisite: prerequisite.map(str::to_owned),
    }
}

fn reverse_domain(domain: &str) -> String {
    domain.split('.').rev().collect::<Vec<_>>().join(".")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(providers: Vec<RegistryNamespaceProvider>) -> RegistryNamespaceRequest {
        RegistryNamespaceRequest {
            brand: "acme-cloud".to_owned(),
            domain: Some("packages.acme.example".to_owned()),
            github_owner: Some("acme-cloud".to_owned()),
            providers,
        }
    }

    fn entry(
        plan: &RegistryNamespacePlan,
        provider: RegistryNamespaceProvider,
    ) -> &RegistryNamespaceEntry {
        plan.entries
            .iter()
            .find(|entry| entry.provider == provider)
            .unwrap()
    }

    #[test]
    fn complete_plan_preserves_each_provider_namespace_model() {
        let plan = plan_registry_namespaces(request(RegistryNamespaceProvider::ALL.to_vec()))
            .unwrap();
        assert_eq!(plan.entries.len(), 7);
        assert_eq!(
            entry(&plan, RegistryNamespaceProvider::Npm).coordinate.as_deref(),
            Some("@acme-cloud")
        );
        assert_eq!(
            entry(&plan, RegistryNamespaceProvider::MavenCentral)
                .coordinate
                .as_deref(),
            Some("example.acme.packages")
        );
        let crates = entry(&plan, RegistryNamespaceProvider::CratesIo);
        assert_eq!(crates.coordinate, None);
        assert_eq!(crates.package_prefix.as_deref(), Some("acme-cloud-"));
        assert_eq!(
            crates.disposition,
            RegistryNamespaceDisposition::NotReservable
        );
        assert_eq!(
            entry(&plan, RegistryNamespaceProvider::PubDev)
                .coordinate
                .as_deref(),
            Some("packages.acme.example")
        );
        for provider in [
            RegistryNamespaceProvider::GitHub,
            RegistryNamespaceProvider::GitLabCom,
            RegistryNamespaceProvider::BitbucketCloud,
        ] {
            assert_eq!(entry(&plan, provider).coordinate.as_deref(), Some("acme-cloud"));
            assert_eq!(
                entry(&plan, provider).disposition,
                RegistryNamespaceDisposition::ManualActionRequired
            );
        }
        plan.validate().unwrap();
    }

    #[test]
    fn maven_uses_explicit_github_fallback_only_without_domain() {
        let mut request = request(vec![RegistryNamespaceProvider::MavenCentral]);
        request.domain = None;
        let plan = plan_registry_namespaces(request).unwrap();
        let maven = &plan.entries[0];
        assert_eq!(maven.coordinate.as_deref(), Some("io.github.acme-cloud"));
        assert!(
            maven
                .proofs
                .contains(&RegistryNamespaceProof::GitHubAccountControl)
        );
        assert!(maven.warnings[0].contains("explicit fallback"));
    }

    #[test]
    fn domain_dependent_providers_fail_closed_without_proof_input() {
        let request = RegistryNamespaceRequest {
            brand: "acme-cloud".to_owned(),
            domain: None,
            github_owner: None,
            providers: vec![
                RegistryNamespaceProvider::MavenCentral,
                RegistryNamespaceProvider::PubDev,
            ],
        };
        let plan = plan_registry_namespaces(request).unwrap();
        for provider in [
            RegistryNamespaceProvider::MavenCentral,
            RegistryNamespaceProvider::PubDev,
        ] {
            let entry = entry(&plan, provider);
            assert_eq!(entry.coordinate, None);
            assert_eq!(
                entry.disposition,
                RegistryNamespaceDisposition::MissingPrerequisite
            );
        }
    }

    #[test]
    fn plans_are_deterministic_across_requested_provider_order() {
        let first = plan_registry_namespaces(request(vec![
            RegistryNamespaceProvider::BitbucketCloud,
            RegistryNamespaceProvider::Npm,
            RegistryNamespaceProvider::CratesIo,
        ]))
        .unwrap();
        let second = plan_registry_namespaces(request(vec![
            RegistryNamespaceProvider::CratesIo,
            RegistryNamespaceProvider::BitbucketCloud,
            RegistryNamespaceProvider::Npm,
        ]))
        .unwrap();
        assert_eq!(
            first.canonical_json_bytes().unwrap(),
            second.canonical_json_bytes().unwrap()
        );
    }
}