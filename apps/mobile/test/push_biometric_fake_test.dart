import 'package:flutter_test/flutter_test.dart';
import 'package:companyos_mobile/src/biometrics/biometric_service.dart';
import 'package:companyos_mobile/src/push/push_service.dart';

void main() {
  test('FakePushService registers without live FCM/APNs', () async {
    final push = FakePushService(token: 'ci-token');
    await push.registerWithBackend(
      register: (token, platform) async {
        expect(token, 'ci-token');
        expect(platform, 'fake');
      },
    );
    expect(push.registrations, hasLength(1));
    push.simulateIncoming(recordId: 'exp_1', orgId: 'org_a');
    expect(push.delivered.single.deepLink, 'companyos://record/exp_1?org=org_a');
  });

  test('FakeBiometricService unlocks without real device biometrics', () async {
    final bio = FakeBiometricService(available: true, shouldSucceed: true);
    expect(await bio.isAvailable, isTrue);
    expect(await bio.authenticate(), isTrue);
    expect(bio.attemptCount, 1);

    final denied = FakeBiometricService(shouldSucceed: false);
    expect(await denied.authenticate(), isFalse);
  });
}
