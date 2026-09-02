import 'package:flutter_test/flutter_test.dart';
import 'package:companyos_mobile/src/crash/crash_reporter.dart';

void main() {
  test('CrashReporter initializes with fake DSN / no-op transport', () async {
    final transport = FakeCrashTransport();
    final reporter = CrashReporter(
      config: const CrashReporterConfig(dsn: 'fake', environment: 'ci'),
      transport: transport,
    );
    expect(reporter.isInitialized, isFalse);
    await reporter.initialize();
    expect(reporter.isInitialized, isTrue);
    expect(transport.initialized, isTrue);
    expect(transport.config?.dsn, 'fake');
    expect(transport.config?.isLiveDsn, isFalse);
  });

  test('empty and ci DSNs are not treated as live', () {
    expect(const CrashReporterConfig(dsn: '').isLiveDsn, isFalse);
    expect(const CrashReporterConfig(dsn: 'noop').isLiveDsn, isFalse);
    expect(const CrashReporterConfig(dsn: 'ci').isLiveDsn, isFalse);
    expect(
      const CrashReporterConfig(dsn: 'https://fake@example.invalid/1').isLiveDsn,
      isFalse,
    );
    expect(
      const CrashReporterConfig(
        dsn: 'https://abc123@o0.ingest.sentry.io/1',
      ).isLiveDsn,
      isTrue,
    );
  });

  test('redacts org ids and PII from messages and context', () async {
    final transport = FakeCrashTransport();
    final reporter = CrashReporter(
      config: const CrashReporterConfig(dsn: 'fake'),
      transport: transport,
    );
    await reporter.initialize();

    await reporter.captureException(
      Exception('fail for org_acme123 user jane@example.com'),
      context: {
        'org_id': 'org_acme123',
        'email': 'jane@example.com',
        'route': '/api/v1/orgs/org_acme123/expenses',
        'note': 'contact +1 555-010-9999',
        'safe': 'ok',
      },
    );

    final event = transport.events.single;
    expect(event.message, contains('[REDACTED_ORG]'));
    expect(event.message, contains('[REDACTED_EMAIL]'));
    expect(event.message, isNot(contains('org_acme123')));
    expect(event.message, isNot(contains('jane@example.com')));
    expect(event.context['org_id'], '[REDACTED]');
    expect(event.context['email'], '[REDACTED]');
    expect(event.context['route'], contains('[REDACTED_ORG]'));
    expect(event.context['note'], contains('[REDACTED_PHONE]'));
    expect(event.context['safe'], 'ok');
  });

  test('redacts bearer tokens and JWTs', () {
    const jwt =
        'eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.signature';
    final text = CrashRedactor.redactText('Authorization: Bearer $jwt');
    expect(text, isNot(contains(jwt)));
    expect(text, contains('[REDACTED_TOKEN]'));
  });

  test('captureMessage requires initialize first', () async {
    final reporter = CrashReporter(
      config: const CrashReporterConfig(dsn: 'fake'),
      transport: FakeCrashTransport(),
    );
    expect(
      () => reporter.captureMessage('boom'),
      throwsA(isA<StateError>()),
    );
  });
}
