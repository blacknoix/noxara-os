import 'dart:convert';

import 'package:flutter/material.dart';

import '../api/api_client.dart';
import '../auth/auth_service.dart';

/// Approvals, tasks, deal quick-updates.
class WorkTab extends StatefulWidget {
  const WorkTab({
    super.key,
    required this.api,
    required this.auth,
    required this.onRefresh,
  });

  final ApiClient api;
  final AuthService auth;
  final Future<void> Function() onRefresh;

  @override
  State<WorkTab> createState() => _WorkTabState();
}

class _WorkTabState extends State<WorkTab> {
  List<dynamic> _approvals = [];
  List<dynamic> _tasks = [];
  List<dynamic> _deals = [];
  String? _error;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    try {
      final a = await widget.api.get('/api/v1/operations/approvals?pending_for_me=true');
      final t = await widget.api.get('/api/v1/operations/tasks?limit=20');
      final d = await widget.api.get('/api/v1/sales/deals?limit=20');
      setState(() {
        if (a.statusCode == 200) {
          _approvals = (jsonDecode(a.body) as Map)['items'] as List<dynamic>? ?? [];
        }
        if (t.statusCode == 200) {
          _tasks = (jsonDecode(t.body) as Map)['items'] as List<dynamic>? ?? [];
        }
        if (d.statusCode == 200) {
          _deals = (jsonDecode(d.body) as Map)['items'] as List<dynamic>? ?? [];
        }
        _error = null;
      });
    } catch (e) {
      setState(() => _error = e.toString());
    }
  }

  Future<void> _decide(String id, bool approve) async {
    await widget.api.post(
      '/api/v1/operations/approvals/$id/decide',
      body: {'approve': approve, 'comment': null},
      idempotencyKey: 'apr-decide-$id-${approve ? 'a' : 'r'}',
    );
    await _load();
  }

  Future<void> _quickDeal(String id, String stageId, int? version) async {
    await widget.api.patch(
      '/api/v1/sales/deals/$id',
      body: {'stage_id': stageId},
      ifMatch: version,
      idempotencyKey: 'deal-quick-$id-$stageId',
    );
    await _load();
  }

  @override
  Widget build(BuildContext context) {
    return RefreshIndicator(
      onRefresh: () async {
        await widget.onRefresh();
        await _load();
      },
      child: ListView(
        physics: const AlwaysScrollableScrollPhysics(),
        padding: const EdgeInsets.all(20),
        children: [
          Text('Work', style: Theme.of(context).textTheme.headlineSmall),
          if (_error != null) Text(_error!, style: const TextStyle(color: Colors.red)),
          const SizedBox(height: 16),
          Text('Approvals', style: Theme.of(context).textTheme.titleMedium),
          ..._approvals.map((raw) {
            final a = raw as Map<String, dynamic>;
            final id = a['id'] as String? ?? '';
            return ListTile(
              title: Text(a['title'] as String? ?? id),
              subtitle: Text(a['status'] as String? ?? ''),
              trailing: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  IconButton(
                    icon: const Icon(Icons.check, color: Color(0xFF0F6E56)),
                    onPressed: () => _decide(id, true),
                  ),
                  IconButton(
                    icon: const Icon(Icons.close, color: Colors.red),
                    onPressed: () => _decide(id, false),
                  ),
                ],
              ),
            );
          }),
          if (_approvals.isEmpty) const Text('No pending approvals'),
          const SizedBox(height: 16),
          Text('Tasks', style: Theme.of(context).textTheme.titleMedium),
          ..._tasks.map((raw) {
            final t = raw as Map<String, dynamic>;
            return ListTile(
              title: Text(t['title'] as String? ?? t['id'] as String? ?? ''),
              subtitle: Text(t['status'] as String? ?? ''),
            );
          }),
          if (_tasks.isEmpty) const Text('No tasks'),
          const SizedBox(height: 16),
          Text('Deals (quick update)', style: Theme.of(context).textTheme.titleMedium),
          ..._deals.map((raw) {
            final d = raw as Map<String, dynamic>;
            final id = d['id'] as String? ?? '';
            final stageId = d['stage_id'] as String? ?? '';
            final version = (d['version'] as num?)?.toInt();
            return ListTile(
              title: Text(d['name'] as String? ?? id),
              subtitle: Text('stage $stageId'),
              trailing: TextButton(
                onPressed: stageId.isEmpty ? null : () => _quickDeal(id, stageId, version),
                child: const Text('Touch'),
              ),
            );
          }),
        ],
      ),
    );
  }
}
