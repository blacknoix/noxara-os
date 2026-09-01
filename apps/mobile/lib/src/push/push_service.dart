/// Push notification interface — live FCM/APNs are out of scope for CI.
abstract class PushService {
  Future<String?> getToken();
  Future<void> registerWithBackend({
    required Future<void> Function(String token, String platform) register,
    String platform = 'fake',
  });
}

/// Fake push used in unit/integration tests (no live keys).
class FakePushService implements PushService {
  FakePushService({this.token = 'fake-push-token-ci'});

  String? token;
  final List<Map<String, String>> registrations = [];
  final List<PushMessage> delivered = [];

  @override
  Future<String?> getToken() async => token;

  @override
  Future<void> registerWithBackend({
    required Future<void> Function(String token, String platform) register,
    String platform = 'fake',
  }) async {
    final t = await getToken();
    if (t == null) return;
    await register(t, platform);
    registrations.add({'token': t, 'platform': platform});
  }

  /// Simulate an incoming push that carries org + record for deep-link routing.
  void simulateIncoming({
    required String recordId,
    required String orgId,
    String? title,
  }) {
    delivered.add(PushMessage(
      title: title ?? 'Update',
      recordId: recordId,
      orgId: orgId,
      deepLink: 'companyos://record/$recordId?org=$orgId',
    ));
  }
}

class PushMessage {
  PushMessage({
    required this.title,
    required this.recordId,
    required this.orgId,
    required this.deepLink,
  });

  final String title;
  final String recordId;
  final String orgId;
  final String deepLink;
}
