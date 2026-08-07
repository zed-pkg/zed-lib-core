// Guards the shape of the exported API, not its behavior.
//
// This package is imported into Flutter apps, where dart:core is implicit and
// an explicit import wins over it. A type exported from here with a dart:core
// name does not produce an ambiguity error — it silently shadows the real one,
// and the consumer gets a baffling error in code that never touched zed-lib.
// That shipped once (a `Comparator` class), so it gets a test.

import 'package:test/test.dart';
import 'package:zed_lib/zed_lib.dart';

void main() {
  test('importing zed_lib does not shadow dart:core types', () {
    // Every one of these would fail to compile if the package exported a type
    // with the same name; the test passing *is* the assertion.
    int byValue(int a, int b) => a.compareTo(b);
    // The type annotation is the point: it must resolve to dart:core's.
    final Comparator<int> comparator = byValue;
    final sorted = [3, 1, 2]..sort(comparator);
    expect(sorted, [1, 2, 3]);

    const duration = Duration(seconds: 1);
    expect(duration.inMilliseconds, 1000);

    final match = RegExp(r'\d+').firstMatch('v12');
    expect(match?.group(0), '12');

    final Set<String> names = {'a'};
    final List<int> numbers = [1];
    final Map<String, int> byName = {'a': 1};
    expect([names.length, numbers.length, byName.length], [1, 1, 1]);
  });

  test('the public surface is the documented one', () {
    // Named so a rename has to be deliberate: these are what consumers import.
    expect(parseVersion('1.2.3')?.minor, 2);
    expect(normalizeCalver('2026.07'), '2026.7.0');
    expect(looksLikeRange('^1.2'), isTrue);
    expect(const VersionBound('>=', SemVer(1, 0, 0)).matches(const SemVer(1, 1, 0)), isTrue);
    expect(resolveRequirement(Requirement.parse('^1'), ['1.4.0']), '1.4.0');
  });
}
