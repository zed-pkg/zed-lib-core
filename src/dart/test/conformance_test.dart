// Runs the shared conformance corpus against the Dart implementation. The Rust
// slice runs the same file; a case that passes in one language and fails in the
// other is the drift this repository exists to catch.

import 'dart:convert';
import 'dart:io';

import 'package:test/test.dart';
import 'package:zed_interfaces/package_metadata.dart';
import 'package:zed_lib/zed_lib.dart';

const _corpusPath = '../../conformance/cases/version-resolution.json';

PackageMetadata metadataFor(String scheme, List<String> versions) =>
    PackageMetadata(
      org: 'acme',
      name: 'conformance',
      vcs: Vcs.git,
      repoUrl: 'https://github.com/acme/conformance',
      latest: versions.isEmpty ? null : versions.last,
      versions: versions,
      versionScheme: switch (scheme) {
        'calver' => VersionScheme.calver,
        'opaque' => VersionScheme.opaque,
        _ => VersionScheme.semver,
      },
    );

void main() {
  final raw = File(_corpusPath).readAsStringSync();
  final corpus = jsonDecode(raw) as Map<String, dynamic>;
  final cases = (corpus['cases'] as List<dynamic>).cast<Map<String, dynamic>>();

  test('the corpus is not empty', () => expect(cases, isNotEmpty));

  for (final testCase in cases) {
    final name = testCase['name'] as String;
    final metadata = metadataFor(
      testCase['scheme'] as String,
      (testCase['versions'] as List<dynamic>).cast<String>(),
    );
    final requirement = testCase['requirement'] as String;
    final expected = testCase['expect'] as Map<String, dynamic>;

    test(name, () {
      final wantVersion = expected['version'] as String?;
      final wantError = expected['error'] as String?;
      expect(
        (wantVersion == null) != (wantError == null),
        isTrue,
        reason: 'a case declares exactly one of `version` or `error`',
      );

      if (wantVersion != null) {
        expect(resolveVersion(metadata, requirement), wantVersion);
      } else {
        try {
          final resolved = resolveVersion(metadata, requirement);
          fail('expected $wantError, resolved $resolved');
        } on ResolveException catch (error) {
          expect(error.kind.wire, wantError);
        }
      }
    });
  }
}
