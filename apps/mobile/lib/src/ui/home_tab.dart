import 'dart:convert';

import 'package:flutter/material.dart';

import '../api/api_client.dart';
import '../auth/auth_service.dart';
import '../offline/read_cache.dart';

class HomeTab extends StatelessWidget {
  const HomeTab({
    super.key,
    required this.api,
    required this.cache,
    required this.auth,
    required this.onRefresh,
    required this.refreshing,
    this.pendingRoute,
    this.statusMessage,
  });

  final ApiClient api;
  final ReadCache cache;
  final AuthService auth;
  final Future<void> Function() onRefresh;
  final bool refreshing;
  final String? pendingRoute;
  final String? statusMessage;

  @override
  Widget build(BuildContext context) {
    final cached = cache.get('/api/v1/dashboard');
    return RefreshIndicator(
      onRefresh: onRefresh,
      child: ListView(
        physics: const AlwaysScrollableScrollPhysics(),
        padding: const EdgeInsets.all(20),
        children: [
          const SizedBox(height: 12),
          Text(
            'CompanyOS',
            style: Theme.of(context).textTheme.headlineMedium?.copyWith(
                  fontWeight: FontWeight.w700,
                  color: const Color(0xFF0F6E56),
                ),
          ),
          Text('Org ${auth.orgId ?? "—"}', style: Theme.of(context).textTheme.bodyMedium),
          if (statusMessage != null) ...[
            const SizedBox(height: 8),
            Text(statusMessage!, style: const TextStyle(color: Color(0xFF0F6E56))),
          ],
          if (pendingRoute != null) ...[
            const SizedBox(height: 4),
            Text('Open: $pendingRoute'),
          ],
          const SizedBox(height: 24),
          Text('Dashboard', style: Theme.of(context).textTheme.titleLarge),
          const SizedBox(height: 8),
          if (refreshing) const LinearProgressIndicator(),
          if (cached != null)
            Text(
              'Cached snapshot (${cached.asOf})\n${const JsonEncoder.withIndent('  ').convert(cached.data)}',
              style: Theme.of(context).textTheme.bodySmall,
            )
          else
            const Text('Pull to refresh — offline cache empty.'),
        ],
      ),
    );
  }
}
