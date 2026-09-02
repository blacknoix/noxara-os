/// Crash reporting behind config. Live DSN is not required in CI.
///
/// Prefer this thin interface + [FakeCrashTransport] over shipping an unused
/// heavy SDK. A real Sentry (or similar) transport can implement [CrashTransport]
/// later without changing call sites.
library;

/// Runtime config from dart-defines / environment.
class CrashReporterConfig {
  const CrashReporterConfig({
    this.dsn = '',
    this.environment = 'ci',
    this.release,
  });

  /// Empty, `fake`, or `noop` → no-op transport (CI default).
  final String dsn;
  final String environment;
  final String? release;

  bool get isLiveDsn {
    final trimmed = dsn.trim();
    if (trimmed.isEmpty) return false;
    final lower = trimmed.toLowerCase();
    if (lower == 'fake' || lower == 'noop' || lower == 'ci') return false;
    if (lower.startsWith('https://fake@') || lower.startsWith('https://ci@')) {
      return false;
    }
    return true;
  }

  /// Build from Flutter `--dart-define=CRASH_DSN=...` (and optional env/release).
  factory CrashReporterConfig.fromEnvironment() {
    return CrashReporterConfig(
      dsn: const String.fromEnvironment('CRASH_DSN', defaultValue: ''),
      environment: const String.fromEnvironment(
        'CRASH_ENVIRONMENT',
        defaultValue: 'ci',
      ),
      release: const String.fromEnvironment('CRASH_RELEASE', defaultValue: '')
              .isEmpty
          ? null
          : const String.fromEnvironment('CRASH_RELEASE'),
    );
  }
}

/// Destination for crash events (real SDK or fake).
abstract class CrashTransport {
  Future<void> initialize(CrashReporterConfig config);
  Future<void> send(CrashEvent event);
}

/// Event after redaction.
class CrashEvent {
  CrashEvent({
    required this.kind,
    required this.message,
    this.stackTrace,
    this.context = const {},
  });

  final CrashEventKind kind;
  final String message;
  final String? stackTrace;
  final Map<String, String> context;
}

enum CrashEventKind { exception, message }

/// No-op / in-memory transport used in CI and when DSN is fake.
class FakeCrashTransport implements CrashTransport {
  FakeCrashTransport();

  CrashReporterConfig? config;
  final List<CrashEvent> events = [];
  bool initialized = false;

  @override
  Future<void> initialize(CrashReporterConfig config) async {
    this.config = config;
    initialized = true;
  }

  @override
  Future<void> send(CrashEvent event) async {
    events.add(event);
  }
}

/// Redacts org identifiers and other PII before transport.
class CrashRedactor {
  static final _orgId = RegExp(r'\borg_[A-Za-z0-9_-]+\b');
  static final _email = RegExp(
    r'[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}',
  );
  static final _jwt = RegExp(
    r'eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+',
  );
  static final _bearer = RegExp(
    r'(bearer\s+)[A-Za-z0-9._+=/-]+',
    caseSensitive: false,
  );
  static final _phone = RegExp(r'\+?\d[\d\s\-().]{7,}\d');

  static const _sensitiveKeys = {
    'email',
    'password',
    'token',
    'access_token',
    'refresh_token',
    'authorization',
    'phone',
    'ssn',
    'name',
    'full_name',
    'org_id',
    'organization_id',
    'user_id',
  };

  static String redactText(String input) {
    var out = input;
    out = out.replaceAll(_jwt, '[REDACTED_TOKEN]');
    out = out.replaceAllMapped(_bearer, (m) => '${m[1]}[REDACTED_TOKEN]');
    out = out.replaceAll(_email, '[REDACTED_EMAIL]');
    out = out.replaceAll(_orgId, '[REDACTED_ORG]');
    out = out.replaceAll(_phone, '[REDACTED_PHONE]');
    return out;
  }

  static Map<String, String> redactContext(Map<String, String>? context) {
    if (context == null || context.isEmpty) return const {};
    final out = <String, String>{};
    for (final entry in context.entries) {
      final key = entry.key.toLowerCase();
      if (_sensitiveKeys.contains(key)) {
        out[entry.key] = '[REDACTED]';
      } else {
        out[entry.key] = redactText(entry.value);
      }
    }
    return out;
  }
}

/// Public reporter used by the app.
class CrashReporter {
  CrashReporter({
    required this.config,
    CrashTransport? transport,
  }) : transport = transport ??
            (config.isLiveDsn
                ? throw StateError(
                    'Live crash DSN set but no CrashTransport provided. '
                    'Wire a real SDK transport or use a fake DSN in CI.',
                  )
                : FakeCrashTransport());

  final CrashReporterConfig config;
  final CrashTransport transport;

  bool _ready = false;

  bool get isInitialized => _ready;

  /// Events captured when using [FakeCrashTransport] (tests / CI).
  List<CrashEvent> get fakeEvents {
    final t = transport;
    if (t is FakeCrashTransport) return t.events;
    return const [];
  }

  Future<void> initialize() async {
    await transport.initialize(config);
    _ready = true;
  }

  Future<void> captureException(
    Object error, {
    StackTrace? stackTrace,
    Map<String, String>? context,
  }) async {
    _ensureReady();
    final event = CrashEvent(
      kind: CrashEventKind.exception,
      message: CrashRedactor.redactText(error.toString()),
      stackTrace: stackTrace == null
          ? null
          : CrashRedactor.redactText(stackTrace.toString()),
      context: CrashRedactor.redactContext(context),
    );
    await transport.send(event);
  }

  Future<void> captureMessage(
    String message, {
    Map<String, String>? context,
  }) async {
    _ensureReady();
    final event = CrashEvent(
      kind: CrashEventKind.message,
      message: CrashRedactor.redactText(message),
      context: CrashRedactor.redactContext(context),
    );
    await transport.send(event);
  }

  void _ensureReady() {
    if (!_ready) {
      throw StateError('CrashReporter.initialize() must be called first');
    }
  }
}
