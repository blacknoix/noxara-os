import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:uuid/uuid.dart';

import '../api/api_client.dart';
import '../auth/auth_service.dart';
import '../biometrics/biometric_service.dart';
import '../offline/mutation_queue.dart';
import '../offline/read_cache.dart';
import 'create_tab.dart';
import 'home_tab.dart';
import 'inbox_tab.dart';
import 'more_tab.dart';
import 'work_tab.dart';

/// Bottom tabs: Home · Work · Create · Inbox · More
class AppShell extends StatefulWidget {
  const AppShell({
    super.key,
    required this.auth,
    required this.api,
    required this.queue,
    required this.cache,
    required this.biometrics,
    this.pendingRoute,
    this.statusMessage,
    this.onDeepLink,
  });

  final AuthService auth;
  final ApiClient api;
  final MutationQueue queue;
  final ReadCache cache;
  final BiometricService biometrics;
  final String? pendingRoute;
  final String? statusMessage;
  final Future<void> Function(String uri)? onDeepLink;

  @override
  State<AppShell> createState() => _AppShellState();
}

class _AppShellState extends State<AppShell> {
  int _index = 0;
  bool _refreshing = false;

  Future<void> _refresh() async {
    setState(() => _refreshing = true);
    try {
      // Replay queued mutations with stable idempotency keys.
      await widget.queue.replay((m) async {
        final res = switch (m.method) {
          'POST' => await widget.api.post(
              m.path,
              body: m.body,
              idempotencyKey: m.idempotencyKey,
            ),
          'PATCH' => await widget.api.patch(
              m.path,
              body: m.body,
              idempotencyKey: m.idempotencyKey,
              ifMatch: m.ifMatch,
            ),
          _ => await widget.api.delete(m.path),
        };
        return TransportResult(statusCode: res.statusCode, body: res.body);
      });
      final dash = await widget.api.get('/api/v1/dashboard');
      if (dash.statusCode == 200) {
        widget.cache.put('/api/v1/dashboard', jsonDecode(dash.body));
      }
    } finally {
      if (mounted) setState(() => _refreshing = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final pages = [
      HomeTab(
        api: widget.api,
        cache: widget.cache,
        auth: widget.auth,
        pendingRoute: widget.pendingRoute,
        statusMessage: widget.statusMessage,
        onRefresh: _refresh,
        refreshing: _refreshing,
      ),
      WorkTab(api: widget.api, auth: widget.auth, onRefresh: _refresh),
      CreateTab(
        queue: widget.queue,
        api: widget.api,
        idGenerator: () => const Uuid().v4(),
      ),
      InboxTab(api: widget.api, onRefresh: _refresh),
      MoreTab(
        auth: widget.auth,
        biometrics: widget.biometrics,
        onDeepLink: widget.onDeepLink,
      ),
    ];

    return Scaffold(
      body: AnimatedSwitcher(
        duration: const Duration(milliseconds: 220),
        switchInCurve: Curves.easeOut,
        switchOutCurve: Curves.easeIn,
        child: KeyedSubtree(
          key: ValueKey(_index),
          child: pages[_index],
        ),
      ),
      bottomNavigationBar: NavigationBar(
        selectedIndex: _index,
        onDestinationSelected: (i) => setState(() => _index = i),
        destinations: const [
          NavigationDestination(icon: Icon(Icons.home_outlined), selectedIcon: Icon(Icons.home), label: 'Home'),
          NavigationDestination(icon: Icon(Icons.work_outline), selectedIcon: Icon(Icons.work), label: 'Work'),
          NavigationDestination(icon: Icon(Icons.add_circle_outline), selectedIcon: Icon(Icons.add_circle), label: 'Create'),
          NavigationDestination(icon: Icon(Icons.inbox_outlined), selectedIcon: Icon(Icons.inbox), label: 'Inbox'),
          NavigationDestination(icon: Icon(Icons.more_horiz), selectedIcon: Icon(Icons.more_horiz), label: 'More'),
        ],
      ),
    );
  }
}
