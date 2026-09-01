import 'package:flutter_test/flutter_test.dart';
import 'package:companyos_mobile/src/deep_link/deep_link.dart';

void main() {
  group('deep links', () {
    test('parses companyos://record/{id}', () {
      final link = DeepLink.parse('companyos://record/exp_01hxyz');
      expect(link, isNotNull);
      expect(link!.recordId, 'exp_01hxyz');
      expect(link.kind, 'expense');
      expect(link.orgId, isNull);
    });

    test('parses org query and path forms', () {
      final q = DeepLink.parse('companyos://record/dl_abc?org=org_acme');
      expect(q!.orgId, 'org_acme');
      expect(q.kind, 'deal');

      final p = DeepLink.parse('companyos://org/org_beta/record/tsk_1');
      expect(p!.orgId, 'org_beta');
      expect(p.recordId, 'tsk_1');
      expect(p.kind, 'task');
    });

    test('opens record in the correct organization (switch when needed)', () async {
      String? current = 'org_a';
      final switched = <String>[];

      final router = DeepLinkRouter(
        currentOrgId: () => current,
        switchOrg: (orgId) async {
          switched.add(orgId);
          current = orgId;
        },
      );

      final link = DeepLink.parse('companyos://record/apr_99?org=org_b')!;
      final nav = await router.open(link);

      expect(switched, ['org_b']);
      expect(nav.orgId, 'org_b');
      expect(nav.recordId, 'apr_99');
      expect(nav.switchedOrg, isTrue);
      expect(nav.route, '/approvals/apr_99');
      expect(current, 'org_b');
    });

    test('does not switch when already in target org', () async {
      var switches = 0;
      final router = DeepLinkRouter(
        currentOrgId: () => 'org_acme',
        switchOrg: (_) async => switches += 1,
      );
      final nav = await router.open(
        DeepLink.parse('companyos://record/exp_1?org=org_acme')!,
      );
      expect(switches, 0);
      expect(nav.switchedOrg, isFalse);
      expect(nav.route, '/finance/expenses/exp_1');
    });

    test('rejects foreign schemes', () {
      expect(DeepLink.parse('https://example.com/record/x'), isNull);
    });
  });
}
