import 'package:flutter/material.dart';

import 'src/api/api_client.dart';
import 'src/auth/auth_service.dart';
import 'src/biometrics/biometric_service.dart';
import 'src/deep_link/deep_link.dart';
import 'src/offline/mutation_queue.dart';
import 'src/offline/read_cache.dart';
import 'src/push/push_service.dart';
import 'src/ui/app_shell.dart';
import 'src/ui/login_screen.dart';

/// Holds the live auth service so [ApiClient] can read the access token.
class SessionHolder {
  static AuthService? auth;
}

void main() {
  WidgetsFlutterBinding.ensureInitialized();
  const apiBase =
      String.fromEnvironment('API_URL', defaultValue: 'http://127.0.0.1:8080');
  final api = ApiClient(
    baseUrl: apiBase,
    getAccessToken: () => SessionHolder.auth?.accessToken,
  );
  final auth = AuthService(api);
  SessionHolder.auth = auth;
  runApp(CompanyOsApp(
    auth: auth,
    api: api,
    push: FakePushService(),
    biometrics: FakeBiometricService(),
    queue: MutationQueue(),
    cache: ReadCache(),
  ));
}

class CompanyOsApp extends StatefulWidget {
  const CompanyOsApp({
    super.key,
    required this.auth,
    required this.api,
    required this.push,
    required this.biometrics,
    required this.queue,
    required this.cache,
  });

  final AuthService auth;
  final ApiClient api;
  final PushService push;
  final BiometricService biometrics;
  final MutationQueue queue;
  final ReadCache cache;

  @override
  State<CompanyOsApp> createState() => _CompanyOsAppState();
}

class _CompanyOsAppState extends State<CompanyOsApp> {
  bool _unlocked = false;
  String? _pendingRoute;
  String? _status;

  @override
  void initState() {
    super.initState();
    _bootstrap();
  }

  Future<void> _bootstrap() async {
    if (widget.auth.session != null) {
      final ok = await widget.biometrics.authenticate();
      if (mounted) setState(() => _unlocked = ok);
    }
  }

  Future<void> _onLoggedIn() async {
    await widget.push.registerWithBackend(
      register: (token, platform) async {
        await widget.api.post('/api/v1/notifications/devices', body: {
          'platform': platform,
          'push_token': token,
          'device_label': 'companyos-mobile',
        });
      },
    );
    if (mounted) setState(() => _unlocked = true);
  }

  Future<void> handleDeepLink(String uri) async {
    final link = DeepLink.parse(uri);
    if (link == null) {
      setState(() => _status = 'ignored deep link');
      return;
    }
    final router = DeepLinkRouter(
      currentOrgId: () => widget.auth.orgId,
      switchOrg: (orgId) async {
        await widget.auth.switchOrg(orgId);
      },
    );
    final nav = await router.open(link);
    if (mounted) {
      setState(() {
        _pendingRoute = nav.route;
        _status = 'opened ${nav.recordId} in ${nav.orgId}';
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final signedIn = widget.auth.session != null && _unlocked;
    return MaterialApp(
      title: 'CompanyOS',
      theme: ThemeData(
        colorScheme: ColorScheme.fromSeed(
          seedColor: const Color(0xFF0F6E56),
          brightness: Brightness.light,
        ),
        useMaterial3: true,
      ),
      home: signedIn
          ? AppShell(
              auth: widget.auth,
              api: widget.api,
              queue: widget.queue,
              cache: widget.cache,
              biometrics: widget.biometrics,
              pendingRoute: _pendingRoute,
              statusMessage: _status,
              onDeepLink: handleDeepLink,
            )
          : LoginScreen(
              auth: widget.auth,
              biometrics: widget.biometrics,
              onSuccess: _onLoggedIn,
            ),
    );
  }
}
