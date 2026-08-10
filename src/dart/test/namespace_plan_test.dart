import 'dart:convert';
import 'dart:io';

import 'package:test/test.dart';
import 'package:zed_interfaces/zed_interfaces.dart';
import 'package:zed_lib/zed_lib.dart';

void main() {
  final corpusFile = File(
    '${Directory.current.path}/../../conformance/cases/registry-namespace-plans.json',
  );
  final corpus =
      jsonDecode(corpusFile.readAsStringSync()) as Map<String, dynamic>;
  final cases = corpus['cases'] as List<dynamic>;

  for (final value in cases) {
    final testCase = value as Map<String, dynamic>;
    test('registry-namespace-plans.json: ${testCase['name']}', () {
      final request = RegistryNamespaceRequest.fromJson(
        testCase['request'] as Map<String, dynamic>,
      );
      final plan = planRegistryNamespaces(request);
      final expected = (testCase['expected'] as List<dynamic>)
          .map((entry) => Map<String, dynamic>.from(entry as Map))
          .toList();
      expect(summarizeRegistryNamespacePlan(plan), equals(expected));
      expect(
        plan.request.providers.map((provider) => provider.toJson()).toList(),
        equals(expected.map((entry) => entry['provider']).toList()),
      );
    });
  }

  test('rejects non-ASCII brand confusables', () {
    expect(
      () => planRegistryNamespaces(
        RegistryNamespaceRequest(
          brand: 'acmе-cloud', // Cyrillic `е`.
          domain: 'acme.example',
          githubOwner: 'acme-cloud',
          providers: const [RegistryNamespaceProvider.npm],
        ),
      ),
      throwsFormatException,
    );
  });

  test('rejects duplicate providers', () {
    expect(
      () => planRegistryNamespaces(
        RegistryNamespaceRequest(
          brand: 'acme-cloud',
          domain: 'acme.example',
          githubOwner: 'acme-cloud',
          providers: const [
            RegistryNamespaceProvider.npm,
            RegistryNamespaceProvider.npm,
          ],
        ),
      ),
      throwsFormatException,
    );
  });
}
