# Registry namespace planning

`planRegistryNamespaces` / `plan_registry_namespaces` turns one validated brand
request into a deterministic `zed.registry-namespace-plan/v1` document. Rust,
TypeScript, and Dart implement the same rules and run
`conformance/cases/registry-namespace-plans.json`.

The planner performs no network request and creates nothing. It describes the
exact provider identity, prerequisites, ordered steps, and warnings that a
later checker or executor must honor.

## Example

Input:

```json
{
  "brand": "acme-cloud",
  "domain": "packages.acme.example",
  "github_owner": "acme-cloud",
  "providers": [
    "npm",
    "maven-central",
    "crates-io",
    "pub-dev",
    "github",
    "gitlab-com",
    "bitbucket-cloud"
  ]
}
```

Key derived identities:

```text
npm                @acme-cloud
Maven Central      example.acme.packages
crates.io           no organization coordinate; advisory acme-cloud- prefix
pub.dev             packages.acme.example
GitHub              acme-cloud
GitLab.com          acme-cloud
Bitbucket Cloud     acme-cloud
```

## Provider rules

### npm

The brand becomes the organization scope `@brand`. Creation remains a manual
npm organization flow and requires account-control evidence.

### Maven Central

A domain is preferred and reversed label-by-label:

```text
packages.acme.example -> example.acme.packages
```

When no domain exists, the planner derives `io.github.<owner>` only from an
explicit `github_owner` request field. It never infers an owner from Git remotes,
credentials, the current process, or a repository URL.

Without either proof input, Maven is `missing-prerequisite` and carries no
coordinate.

### crates.io

No organization namespace is represented. The planner may suggest `brand-` as a
consistent crate-name prefix, but the entry remains `not-reservable`. Each real
crate name is checked and acquired independently through its first genuine
publication; placeholder squatting is explicitly warned against.

### pub.dev

The publisher coordinate is the supplied canonical domain. Without a domain,
the entry is `missing-prerequisite`. Package names remain global even when they
are associated with a verified publisher.

### GitHub, GitLab.com, and Bitbucket Cloud

The portable brand is reused as organization, top-level group, or workspace
coordinate. The planner marks creation as manual and records a race warning:
read-only availability is not a reservation.

## Execution boundary

A later availability checker should attach observations to the canonical plan
digest without changing coordinates. A mutating executor should:

1. require explicit consent for exactly one provider step;
2. accept credentials through an explicit provider-specific channel;
3. avoid logging tokens or challenge material;
4. perform one mutation;
5. re-read provider state independently; and
6. produce `zed.registry-namespace-claim-receipt/v1` evidence.

The executor must stop on a taken coordinate instead of silently selecting a
suffix. Cross-registry naming is valuable only when the external identity stays
intentional and auditable.