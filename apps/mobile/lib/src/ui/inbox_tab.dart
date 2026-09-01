import 'dart:convert';

import 'package:flutter/material.dart';

import '../api/api_client.dart';

class InboxTab extends StatefulWidget {
  const InboxTab({super.key, required this.api, required this.onRefresh});

  final ApiClient api;
  final Future<void> Function() onRefresh;

  @override
  State<InboxTab> createState() => _InboxTabState();
}

class _InboxTabState extends State<InboxTab> {
  List<dynamic> _items = [];

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    final res = await widget.api.get('/api/v1/notifications/feed');
    if (res.statusCode == 200) {
      final body = jsonDecode(res.body) as Map<String, dynamic>;
      setState(() => _items = body['items'] as List<dynamic>? ?? []);
    }
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
          Text('Inbox', style: Theme.of(context).textTheme.headlineSmall),
          const SizedBox(height: 12),
          if (_items.isEmpty) const Text('No notifications'),
          ..._items.map((raw) {
            final n = raw as Map<String, dynamic>;
            return ListTile(
              title: Text(n['title'] as String? ?? ''),
              subtitle: Text(n['body'] as String? ?? ''),
            );
          }),
        ],
      ),
    );
  }
}
