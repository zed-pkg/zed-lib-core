import type {
  RegistryNamespaceAction,
  RegistryNamespaceEntry,
  RegistryNamespacePlan,
  RegistryNamespaceProof,
  RegistryNamespaceProvider,
  RegistryNamespaceRequest,
  RegistryNamespaceStep,
} from "@zed-pkg/zed-interfaces";

const providerOrder: readonly RegistryNamespaceProvider[] = [
  "npm",
  "maven-central",
  "crates-io",
  "pub-dev",
  "github",
  "gitlab-com",
  "bitbucket-cloud",
];

const planWarnings = [
  "Provider availability can change between planning, manual proof, and claim execution.",
  "This plan is pre-mutation intent and is not external namespace ownership evidence.",
] as const;

export function planRegistryNamespaces(
  input: RegistryNamespaceRequest,
): RegistryNamespacePlan {
  validateRequest(input);
  const providers = [...input.providers].sort(
    (left, right) => providerOrder.indexOf(left) - providerOrder.indexOf(right),
  );
  const request: RegistryNamespaceRequest = { ...input, providers };
  return {
    schema: "zed.registry-namespace-plan/v1",
    request,
    entries: providers.map((provider) => planProvider(request, provider)),
    warnings: [...planWarnings],
  };
}

function planProvider(
  request: RegistryNamespaceRequest,
  provider: RegistryNamespaceProvider,
): RegistryNamespaceEntry {
  switch (provider) {
    case "npm":
      return npm(request);
    case "maven-central":
      return mavenCentral(request);
    case "crates-io":
      return cratesIo(request);
    case "pub-dev":
      return pubDev(request);
    case "github":
      return forge(
        provider,
        "forge-organization",
        "create-organization",
        "Create the GitHub organization through the account-owned organization flow.",
        request,
      );
    case "gitlab-com":
      return forge(
        provider,
        "forge-group",
        "create-group",
        "Create the GitLab.com top-level group through the account-owned group flow.",
        request,
      );
    case "bitbucket-cloud":
      return forge(
        provider,
        "forge-workspace",
        "create-workspace",
        "Create the Bitbucket Cloud workspace through Atlassian Administration.",
        request,
      );
  }
}

function npm(request: RegistryNamespaceRequest): RegistryNamespaceEntry {
  const coordinate = `@${request.brand}`;
  return {
    provider: "npm",
    model: "literal-organization-scope",
    coordinate,
    automation: "manual-web-flow",
    disposition: "manual-action-required",
    proofs: ["registry-account-control"],
    steps: [
      step(
        "check-availability",
        `Check whether npm organization scope \`${coordinate}\` is available.`,
      ),
      step(
        "create-organization",
        `Create npm organization \`${request.brand}\` so the matching scope is organization-owned.`,
        true,
        "Control an npm account authorized to create an organization.",
      ),
      step(
        "record-ownership-evidence",
        `Re-read npm organization \`${request.brand}\` and record non-secret ownership evidence.`,
        false,
        "The organization exists and the acting account is an owner.",
      ),
    ],
    warnings: [
      "Unscoped npm package names are global and are not protected by this organization claim.",
    ],
  };
}

function mavenCentral(
  request: RegistryNamespaceRequest,
): RegistryNamespaceEntry {
  if (request.domain != null) {
    const coordinate = reverseDomain(request.domain);
    return {
      provider: "maven-central",
      model: "verified-group-id-prefix",
      coordinate,
      automation: "manual-web-flow",
      disposition: "manual-action-required",
      proofs: ["registry-account-control", "domain-control"],
      steps: [
        step(
          "check-availability",
          `Check whether Maven Central namespace \`${coordinate}\` is already registered.`,
        ),
        step(
          "register-namespace",
          `Register Maven Central namespace \`${coordinate}\` in Central Portal.`,
          true,
          "Control a Central Portal publishing account.",
        ),
        step(
          "verify-domain",
          `Complete the provider challenge proving control of \`${request.domain}\`.`,
          true,
          "Control DNS or another provider-approved proof channel.",
        ),
        step(
          "record-ownership-evidence",
          `Re-read verified Maven namespace \`${coordinate}\` and record non-secret evidence.`,
        ),
      ],
      warnings: [
        "A derived reverse-DNS groupId is only a candidate until Maven Central accepts the proof.",
      ],
    };
  }

  if (request.github_owner != null) {
    const coordinate = `io.github.${request.github_owner}`;
    return {
      provider: "maven-central",
      model: "verified-group-id-prefix",
      coordinate,
      automation: "manual-web-flow",
      disposition: "manual-action-required",
      proofs: ["registry-account-control", "github-account-control"],
      steps: [
        step(
          "check-availability",
          `Check whether Maven Central namespace \`${coordinate}\` is already registered.`,
        ),
        step(
          "register-namespace",
          `Register Maven Central namespace \`${coordinate}\` in Central Portal.`,
          true,
          "Control a Central Portal publishing account.",
        ),
        step(
          "record-ownership-evidence",
          `Complete GitHub-owner proof for \`${request.github_owner}\` and record the verified namespace.`,
          true,
          "Control the explicitly named GitHub owner; ambient Git credentials are not proof.",
        ),
      ],
      warnings: [
        "The `io.github` coordinate is an explicit fallback, not a substitute for a controlled product domain.",
      ],
    };
  }

  return {
    provider: "maven-central",
    model: "verified-group-id-prefix",
    automation: "manual-web-flow",
    disposition: "missing-prerequisite",
    proofs: ["domain-control", "github-account-control"],
    steps: [
      step(
        "register-namespace",
        "Supply a controlled domain or an explicit GitHub owner before deriving a Maven namespace.",
        false,
        "A canonical domain is preferred; an explicit GitHub owner enables the `io.github` fallback.",
      ),
    ],
    warnings: [
      "No Maven coordinate was derived because neither domain nor explicit GitHub owner was supplied.",
    ],
  };
}

function cratesIo(request: RegistryNamespaceRequest): RegistryNamespaceEntry {
  const packagePrefix = `${request.brand}-`;
  return {
    provider: "crates-io",
    model: "global-package-names",
    package_prefix: packagePrefix,
    automation: "not-reservable",
    disposition: "not-reservable",
    proofs: ["existing-package-ownership"],
    steps: [
      step(
        "check-availability",
        `Check every intended crates.io name using advisory prefix \`${packagePrefix}\`.`,
      ),
      step(
        "publish-first-package",
        "Publish each genuine crate to acquire that individual global crate name.",
        false,
        "The crate is release-ready and complies with crates.io publication policy.",
      ),
      step(
        "add-owner-team",
        "Add intended GitHub users or a team as crate owners after publication.",
      ),
      step(
        "record-ownership-evidence",
        "Record non-secret ownership evidence for each individual crate name.",
      ),
    ],
    warnings: [
      `\`${packagePrefix}\` is a naming convention only; crates.io does not reserve organization prefixes.`,
      "Do not publish empty placeholder crates solely to squat on names.",
    ],
  };
}

function pubDev(request: RegistryNamespaceRequest): RegistryNamespaceEntry {
  if (request.domain == null) {
    return {
      provider: "pub-dev",
      model: "verified-publisher-domain",
      automation: "manual-web-flow",
      disposition: "missing-prerequisite",
      proofs: ["domain-control"],
      steps: [
        step(
          "verify-domain",
          "Supply and prove control of a canonical domain before creating a pub.dev publisher.",
          true,
          "A verified publisher is domain-derived; a brand slug alone is insufficient.",
        ),
      ],
      warnings: [
        "No pub.dev publisher coordinate was derived because no domain was supplied.",
      ],
    };
  }

  return {
    provider: "pub-dev",
    model: "verified-publisher-domain",
    coordinate: request.domain,
    automation: "manual-web-flow",
    disposition: "manual-action-required",
    proofs: ["registry-account-control", "domain-control"],
    steps: [
      step(
        "verify-domain",
        `Prove control of \`${request.domain}\` through the pub.dev publisher flow.`,
        true,
        "Control the domain verification channel and a pub.dev-linked account.",
      ),
      step(
        "create-publisher",
        `Create verified pub.dev publisher \`${request.domain}\`.`,
        true,
        "pub.dev accepts the domain-control proof.",
      ),
      step(
        "record-ownership-evidence",
        `Re-read publisher \`${request.domain}\` and record non-secret verification evidence.`,
      ),
    ],
    warnings: [
      "pub.dev package names remain global even when a package is associated with a verified publisher.",
    ],
  };
}

function forge(
  provider: RegistryNamespaceProvider,
  model: RegistryNamespaceEntry["model"],
  action: RegistryNamespaceAction,
  createSummary: string,
  request: RegistryNamespaceRequest,
): RegistryNamespaceEntry {
  return {
    provider,
    model,
    coordinate: request.brand,
    automation: "manual-web-flow",
    disposition: "manual-action-required",
    proofs: ["forge-administrator"],
    steps: [
      step(
        "check-availability",
        `Check whether provider coordinate \`${request.brand}\` is currently available.`,
      ),
      step(
        action,
        createSummary,
        true,
        "Use an account authorized to create and administer the provider entity.",
      ),
      step(
        "record-ownership-evidence",
        `Re-read \`${request.brand}\` and record non-secret administrator evidence.`,
      ),
    ],
    warnings: [
      "A read-only availability result does not reserve the coordinate and may race another claimant.",
    ],
  };
}

function step(
  action: RegistryNamespaceAction,
  summary: string,
  manual = false,
  prerequisite?: string,
): RegistryNamespaceStep {
  return { action, summary, manual, ...(prerequisite == null ? {} : { prerequisite }) };
}

function reverseDomain(domain: string): string {
  return domain.split(".").reverse().join(".");
}

function validateRequest(request: RegistryNamespaceRequest): void {
  if (!/^[a-z0-9](?:[a-z0-9-]{0,37}[a-z0-9])?$/.test(request.brand) || request.brand.includes("--")) {
    throw new Error(`invalid portable brand slug: ${request.brand}`);
  }
  if (request.domain != null && !isDomain(request.domain)) {
    throw new Error(`invalid canonical domain: ${request.domain}`);
  }
  if (
    request.github_owner != null &&
    (!/^[a-z0-9](?:[a-z0-9-]{0,37}[a-z0-9])?$/.test(request.github_owner) ||
      request.github_owner.includes("--"))
  ) {
    throw new Error(`invalid explicit GitHub owner: ${request.github_owner}`);
  }
  if (request.providers.length === 0) {
    throw new Error("at least one provider is required");
  }
  if (new Set(request.providers).size !== request.providers.length) {
    throw new Error("duplicate registry namespace provider");
  }
}

function isDomain(domain: string): boolean {
  return (
    domain.length <= 253 &&
    domain === domain.toLowerCase() &&
    domain.includes(".") &&
    !domain.endsWith(".") &&
    domain.split(".").every((label) =>
      /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/.test(label),
    )
  );
}

export function summarizeRegistryNamespacePlan(plan: RegistryNamespacePlan) {
  return plan.entries.map((entry) => ({
    provider: entry.provider,
    coordinate: entry.coordinate ?? null,
    package_prefix: entry.package_prefix ?? null,
    automation: entry.automation,
    disposition: entry.disposition,
    proofs: [...entry.proofs],
    step_actions: entry.steps.map((step) => step.action),
  }));
}

export type RegistryNamespacePlanSummary = ReturnType<
  typeof summarizeRegistryNamespacePlan
>;

// Keep the imported proof type in the public compile-time surface so provider
// additions cannot silently lose proof compatibility.
export type RegistryNamespacePlanProof = RegistryNamespaceProof;
