import 'package:zed_interfaces/zed_interfaces.dart';

const _providerOrder = <RegistryNamespaceProvider>[
  RegistryNamespaceProvider.npm,
  RegistryNamespaceProvider.mavenCentral,
  RegistryNamespaceProvider.cratesIo,
  RegistryNamespaceProvider.pubDev,
  RegistryNamespaceProvider.github,
  RegistryNamespaceProvider.gitlabCom,
  RegistryNamespaceProvider.bitbucketCloud,
];

RegistryNamespacePlan planRegistryNamespaces(
  RegistryNamespaceRequest input,
) {
  _validateRequest(input);
  final providers = [...input.providers]..sort(
      (left, right) =>
          _providerOrder.indexOf(left).compareTo(_providerOrder.indexOf(right)),
    );
  final request = RegistryNamespaceRequest(
    brand: input.brand,
    domain: input.domain,
    githubOwner: input.githubOwner,
    providers: providers,
  );
  return RegistryNamespacePlan(
    schema: 'zed.registry-namespace-plan/v1',
    request: request,
    entries:
        providers.map((provider) => _planProvider(request, provider)).toList(),
    warnings: const [
      'Provider availability can change between planning, manual proof, and claim execution.',
      'This plan is pre-mutation intent and is not external namespace ownership evidence.',
    ],
  );
}

RegistryNamespaceEntry _planProvider(
  RegistryNamespaceRequest request,
  RegistryNamespaceProvider provider,
) {
  switch (provider) {
    case RegistryNamespaceProvider.npm:
      return _npm(request);
    case RegistryNamespaceProvider.mavenCentral:
      return _mavenCentral(request);
    case RegistryNamespaceProvider.cratesIo:
      return _cratesIo(request);
    case RegistryNamespaceProvider.pubDev:
      return _pubDev(request);
    case RegistryNamespaceProvider.github:
      return _forge(
        provider,
        RegistryNamespaceModel.forgeOrganization,
        RegistryNamespaceAction.createOrganization,
        'Create the GitHub organization through the account-owned organization flow.',
        request,
      );
    case RegistryNamespaceProvider.gitlabCom:
      return _forge(
        provider,
        RegistryNamespaceModel.forgeGroup,
        RegistryNamespaceAction.createGroup,
        'Create the GitLab.com top-level group through the account-owned group flow.',
        request,
      );
    case RegistryNamespaceProvider.bitbucketCloud:
      return _forge(
        provider,
        RegistryNamespaceModel.forgeWorkspace,
        RegistryNamespaceAction.createWorkspace,
        'Create the Bitbucket Cloud workspace through Atlassian Administration.',
        request,
      );
  }
}

RegistryNamespaceEntry _npm(RegistryNamespaceRequest request) {
  final coordinate = '@${request.brand}';
  return RegistryNamespaceEntry(
    provider: RegistryNamespaceProvider.npm,
    model: RegistryNamespaceModel.literalOrganizationScope,
    coordinate: coordinate,
    automation: RegistryNamespaceAutomation.manualWebFlow,
    disposition: RegistryNamespaceDisposition.manualActionRequired,
    proofs: const [RegistryNamespaceProof.registryAccountControl],
    steps: [
      _step(
        RegistryNamespaceAction.checkAvailability,
        'Check whether npm organization scope `$coordinate` is available.',
      ),
      _step(
        RegistryNamespaceAction.createOrganization,
        'Create npm organization `${request.brand}` so the matching scope is organization-owned.',
        manual: true,
        prerequisite:
            'Control an npm account authorized to create an organization.',
      ),
      _step(
        RegistryNamespaceAction.recordOwnershipEvidence,
        'Re-read npm organization `${request.brand}` and record non-secret ownership evidence.',
        prerequisite:
            'The organization exists and the acting account is an owner.',
      ),
    ],
    warnings: const [
      'Unscoped npm package names are global and are not protected by this organization claim.',
    ],
  );
}

RegistryNamespaceEntry _mavenCentral(RegistryNamespaceRequest request) {
  if (request.domain != null) {
    final coordinate = _reverseDomain(request.domain!);
    return RegistryNamespaceEntry(
      provider: RegistryNamespaceProvider.mavenCentral,
      model: RegistryNamespaceModel.verifiedGroupIdPrefix,
      coordinate: coordinate,
      automation: RegistryNamespaceAutomation.manualWebFlow,
      disposition: RegistryNamespaceDisposition.manualActionRequired,
      proofs: const [
        RegistryNamespaceProof.registryAccountControl,
        RegistryNamespaceProof.domainControl,
      ],
      steps: [
        _step(
          RegistryNamespaceAction.checkAvailability,
          'Check whether Maven Central namespace `$coordinate` is already registered.',
        ),
        _step(
          RegistryNamespaceAction.registerNamespace,
          'Register Maven Central namespace `$coordinate` in Central Portal.',
          manual: true,
          prerequisite: 'Control a Central Portal publishing account.',
        ),
        _step(
          RegistryNamespaceAction.verifyDomain,
          'Complete the provider challenge proving control of `${request.domain}`.',
          manual: true,
          prerequisite:
              'Control DNS or another provider-approved proof channel.',
        ),
        _step(
          RegistryNamespaceAction.recordOwnershipEvidence,
          'Re-read verified Maven namespace `$coordinate` and record non-secret evidence.',
        ),
      ],
      warnings: const [
        'A derived reverse-DNS groupId is only a candidate until Maven Central accepts the proof.',
      ],
    );
  }

  if (request.githubOwner != null) {
    final coordinate = 'io.github.${request.githubOwner}';
    return RegistryNamespaceEntry(
      provider: RegistryNamespaceProvider.mavenCentral,
      model: RegistryNamespaceModel.verifiedGroupIdPrefix,
      coordinate: coordinate,
      automation: RegistryNamespaceAutomation.manualWebFlow,
      disposition: RegistryNamespaceDisposition.manualActionRequired,
      proofs: const [
        RegistryNamespaceProof.registryAccountControl,
        RegistryNamespaceProof.githubAccountControl,
      ],
      steps: [
        _step(
          RegistryNamespaceAction.checkAvailability,
          'Check whether Maven Central namespace `$coordinate` is already registered.',
        ),
        _step(
          RegistryNamespaceAction.registerNamespace,
          'Register Maven Central namespace `$coordinate` in Central Portal.',
          manual: true,
          prerequisite: 'Control a Central Portal publishing account.',
        ),
        _step(
          RegistryNamespaceAction.recordOwnershipEvidence,
          'Complete GitHub-owner proof for `${request.githubOwner}` and record the verified namespace.',
          manual: true,
          prerequisite:
              'Control the explicitly named GitHub owner; ambient Git credentials are not proof.',
        ),
      ],
      warnings: const [
        'The `io.github` coordinate is an explicit fallback, not a substitute for a controlled product domain.',
      ],
    );
  }

  return RegistryNamespaceEntry(
    provider: RegistryNamespaceProvider.mavenCentral,
    model: RegistryNamespaceModel.verifiedGroupIdPrefix,
    automation: RegistryNamespaceAutomation.manualWebFlow,
    disposition: RegistryNamespaceDisposition.missingPrerequisite,
    proofs: const [
      RegistryNamespaceProof.domainControl,
      RegistryNamespaceProof.githubAccountControl,
    ],
    steps: [
      _step(
        RegistryNamespaceAction.registerNamespace,
        'Supply a controlled domain or an explicit GitHub owner before deriving a Maven namespace.',
        prerequisite:
            'A canonical domain is preferred; an explicit GitHub owner enables the `io.github` fallback.',
      ),
    ],
    warnings: const [
      'No Maven coordinate was derived because neither domain nor explicit GitHub owner was supplied.',
    ],
  );
}

RegistryNamespaceEntry _cratesIo(RegistryNamespaceRequest request) {
  final prefix = '${request.brand}-';
  return RegistryNamespaceEntry(
    provider: RegistryNamespaceProvider.cratesIo,
    model: RegistryNamespaceModel.globalPackageNames,
    packagePrefix: prefix,
    automation: RegistryNamespaceAutomation.notReservable,
    disposition: RegistryNamespaceDisposition.notReservable,
    proofs: const [RegistryNamespaceProof.existingPackageOwnership],
    steps: [
      _step(
        RegistryNamespaceAction.checkAvailability,
        'Check every intended crates.io name using advisory prefix `$prefix`.',
      ),
      _step(
        RegistryNamespaceAction.publishFirstPackage,
        'Publish each genuine crate to acquire that individual global crate name.',
        prerequisite:
            'The crate is release-ready and complies with crates.io publication policy.',
      ),
      _step(
        RegistryNamespaceAction.addOwnerTeam,
        'Add intended GitHub users or a team as crate owners after publication.',
      ),
      _step(
        RegistryNamespaceAction.recordOwnershipEvidence,
        'Record non-secret ownership evidence for each individual crate name.',
      ),
    ],
    warnings: [
      '`$prefix` is a naming convention only; crates.io does not reserve organization prefixes.',
      'Do not publish empty placeholder crates solely to squat on names.',
    ],
  );
}

RegistryNamespaceEntry _pubDev(RegistryNamespaceRequest request) {
  if (request.domain == null) {
    return RegistryNamespaceEntry(
      provider: RegistryNamespaceProvider.pubDev,
      model: RegistryNamespaceModel.verifiedPublisherDomain,
      automation: RegistryNamespaceAutomation.manualWebFlow,
      disposition: RegistryNamespaceDisposition.missingPrerequisite,
      proofs: const [RegistryNamespaceProof.domainControl],
      steps: [
        _step(
          RegistryNamespaceAction.verifyDomain,
          'Supply and prove control of a canonical domain before creating a pub.dev publisher.',
          manual: true,
          prerequisite:
              'A verified publisher is domain-derived; a brand slug alone is insufficient.',
        ),
      ],
      warnings: const [
        'No pub.dev publisher coordinate was derived because no domain was supplied.',
      ],
    );
  }

  return RegistryNamespaceEntry(
    provider: RegistryNamespaceProvider.pubDev,
    model: RegistryNamespaceModel.verifiedPublisherDomain,
    coordinate: request.domain,
    automation: RegistryNamespaceAutomation.manualWebFlow,
    disposition: RegistryNamespaceDisposition.manualActionRequired,
    proofs: const [
      RegistryNamespaceProof.registryAccountControl,
      RegistryNamespaceProof.domainControl,
    ],
    steps: [
      _step(
        RegistryNamespaceAction.verifyDomain,
        'Prove control of `${request.domain}` through the pub.dev publisher flow.',
        manual: true,
        prerequisite:
            'Control the domain verification channel and a pub.dev-linked account.',
      ),
      _step(
        RegistryNamespaceAction.createPublisher,
        'Create verified pub.dev publisher `${request.domain}`.',
        manual: true,
        prerequisite: 'pub.dev accepts the domain-control proof.',
      ),
      _step(
        RegistryNamespaceAction.recordOwnershipEvidence,
        'Re-read publisher `${request.domain}` and record non-secret verification evidence.',
      ),
    ],
    warnings: const [
      'pub.dev package names remain global even when a package is associated with a verified publisher.',
    ],
  );
}

RegistryNamespaceEntry _forge(
  RegistryNamespaceProvider provider,
  RegistryNamespaceModel model,
  RegistryNamespaceAction action,
  String createSummary,
  RegistryNamespaceRequest request,
) =>
    RegistryNamespaceEntry(
      provider: provider,
      model: model,
      coordinate: request.brand,
      automation: RegistryNamespaceAutomation.manualWebFlow,
      disposition: RegistryNamespaceDisposition.manualActionRequired,
      proofs: const [RegistryNamespaceProof.forgeAdministrator],
      steps: [
        _step(
          RegistryNamespaceAction.checkAvailability,
          'Check whether provider coordinate `${request.brand}` is currently available.',
        ),
        _step(
          action,
          createSummary,
          manual: true,
          prerequisite:
              'Use an account authorized to create and administer the provider entity.',
        ),
        _step(
          RegistryNamespaceAction.recordOwnershipEvidence,
          'Re-read `${request.brand}` and record non-secret administrator evidence.',
        ),
      ],
      warnings: const [
        'A read-only availability result does not reserve the coordinate and may race another claimant.',
      ],
    );

RegistryNamespaceStep _step(
  RegistryNamespaceAction action,
  String summary, {
  bool manual = false,
  String? prerequisite,
}) =>
    RegistryNamespaceStep(
      action: action,
      summary: summary,
      manual: manual,
      prerequisite: prerequisite,
    );

String _reverseDomain(String domain) => domain.split('.').reversed.join('.');

void _validateRequest(RegistryNamespaceRequest request) {
  final slug = RegExp(r'^[a-z0-9](?:[a-z0-9-]{0,37}[a-z0-9])?$');
  if (!slug.hasMatch(request.brand) || request.brand.contains('--')) {
    throw FormatException('invalid portable brand slug: ${request.brand}');
  }
  if (request.domain != null && !_isDomain(request.domain!)) {
    throw FormatException('invalid canonical domain: ${request.domain}');
  }
  if (request.githubOwner != null &&
      (!slug.hasMatch(request.githubOwner!) ||
          request.githubOwner!.contains('--'))) {
    throw FormatException(
        'invalid explicit GitHub owner: ${request.githubOwner}');
  }
  if (request.providers.isEmpty) {
    throw const FormatException('at least one provider is required');
  }
  if (request.providers.toSet().length != request.providers.length) {
    throw const FormatException('duplicate registry namespace provider');
  }
}

bool _isDomain(String domain) =>
    domain.length <= 253 &&
    domain == domain.toLowerCase() &&
    domain.contains('.') &&
    !domain.endsWith('.') &&
    domain.split('.').every(
          (label) => RegExp(
            r'^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$',
          ).hasMatch(label),
        );

List<Map<String, Object?>> summarizeRegistryNamespacePlan(
  RegistryNamespacePlan plan,
) =>
    plan.entries
        .map(
          (entry) => <String, Object?>{
            'provider': entry.provider.toJson(),
            'coordinate': entry.coordinate,
            'package_prefix': entry.packagePrefix,
            'automation': entry.automation.toJson(),
            'disposition': entry.disposition.toJson(),
            'proofs': (entry.proofs ?? const <RegistryNamespaceProof>[])
                .map((proof) => proof.toJson())
                .toList(),
            'step_actions':
                entry.steps.map((step) => step.action.toJson()).toList(),
          },
        )
        .toList();
