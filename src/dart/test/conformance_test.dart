// Runs every corpus file in ../../conformance/cases against the Dart
// implementation. The Rust and TypeScript slices load the same directory.
//
// Most of these cases were answered by Rust and written out by
// `cargo run --example generate_fuzz_corpus`: Rust delegates the hard part to
// the same `semver` crate Cargo uses, so what is really under test here is
// whether this hand-written algebra agrees with Cargo across combinations
// nobody would think to write down by hand.

import 'dart:convert';
import 'dart:io';

import 'package:test/test.dart';
import 'package:zed_interfaces/package_metadata.dart';
import 'package:zed_lib/zed_lib.dart';

const _casesDir = '../../conformance/cases';
const _resolutionSchema = 'zed-lib/conformance/version-resolution/v1';
const _latestSchema = 'zed-lib/conformance/latest-stable/v1';

/// `latest` is data for a latest-stable case — including when it is null, which
/// is what "the registry recorded nothing" looks like. Resolution cases never
/// read it, so they get a convenient fallback instead.
PackageMetadata metadataFor(
  String scheme,
  List<String> versions, {
  required String? latest,
  required bool latestIsData,
}) => PackageMetadata(
  org: 'acme',
  name: 'conformance',
  vcs: Vcs.git,
  repoUrl: 'https://github.com/acme/conformance',
  latest: latestIsData ? latest : (latest ?? (versions.isEmpty ? null : versions.last)),
  versions: versions,
  versionScheme: switch (scheme) {
    'calver' => VersionScheme.calver,
    'opaque' => VersionScheme.opaque,
    _ => VersionScheme.semver,
  },
);

void main() {
  final files = Directory(_casesDir)
      .listSync()
      .whereType<File>()
      .where((file) => file.path.endsWith('.json'))
      .toList()
    ..sort((a, b) => a.path.compareTo(b.path));

  test('the corpus directory is not empty', () => expect(files, isNotEmpty));

  var total = 0;

  for (final file in files) {
    final name = file.uri.pathSegments.last;
    final corpus = jsonDecode(file.readAsStringSync()) as Map<String, dynamic>;
    final schema = corpus['schema'] as String;
    final cases = (corpus['cases'] as List<dynamic>).cast<Map<String, dynamic>>();

    group(name, () {
      for (final testCase in cases) {
        final caseName = testCase['name'] as String;
        final versions = (testCase['versions'] as List<dynamic>).cast<String>();
        final expected = testCase['expect'] as Map<String, dynamic>;
        final wantVersion = expected['version'] as String?;
        total++;

        switch (schema) {
          case _resolutionSchema:
            test(caseName, () {
              final metadata = metadataFor(
                testCase['scheme'] as String,
                versions,
                latest: testCase['latest'] as String?,
                latestIsData: false,
              );
              final requirement = testCase['requirement'] as String;
              final wantError = expected['error'] as String?;
              expect(
                (wantVersion == null) != (wantError == null),
                isTrue,
                reason: 'declare exactly one of `version` or `error`',
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

          case _latestSchema:
            test(caseName, () {
              final metadata = metadataFor(
                testCase['scheme'] as String,
                versions,
                latest: testCase['latest'] as String?,
                latestIsData: true,
              );
              expect(expected['error'], isNull, reason: 'latest-stable cases return null, not errors');
              expect(latestStable(metadata), wantVersion);
            });

          default:
            test('$name has a known schema', () => fail('unknown corpus schema `$schema`'));
        }
      }
    });
  }

  // A loader bug that silently matched nothing would look like a clean run.
  test('the generated corpus was loaded too', () {
    expect(total, greaterThan(100), reason: 'ran only $total cases');
  });
}
