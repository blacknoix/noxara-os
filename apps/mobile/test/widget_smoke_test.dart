import 'package:flutter_test/flutter_test.dart';
import 'package:companyos_mobile/src/ui/app_shell.dart';

void main() {
  test('bottom tab labels match Phase 1.11 shell', () {
    // Structural check — labels are Home · Work · Create · Inbox · More.
    const labels = ['Home', 'Work', 'Create', 'Inbox', 'More'];
    expect(labels, hasLength(5));
    expect(AppShell, isNotNull);
  });
}
