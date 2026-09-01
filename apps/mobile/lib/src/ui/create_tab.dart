import 'package:flutter/material.dart';

import '../api/api_client.dart';
import '../offline/mutation_queue.dart';

/// Camera-first expense capture — queues offline with stable Idempotency-Key.
class CreateTab extends StatefulWidget {
  const CreateTab({
    super.key,
    required this.queue,
    required this.api,
    required this.idGenerator,
  });

  final MutationQueue queue;
  final ApiClient api;
  final String Function() idGenerator;

  @override
  State<CreateTab> createState() => _CreateTabState();
}

class _CreateTabState extends State<CreateTab> {
  final _desc = TextEditingController();
  final _amount = TextEditingController(text: '12.00');
  final _currency = TextEditingController(text: 'USD');
  String? _message;
  bool _forceOffline = false;

  int _parseMinor(String raw) {
    final cleaned = raw.trim();
    if (cleaned.contains('.')) {
      final parts = cleaned.split('.');
      final whole = int.parse(parts[0].isEmpty ? '0' : parts[0]);
      final frac =
          (parts.length > 1 ? parts[1] : '0').padRight(2, '0').substring(0, 2);
      return whole * 100 + int.parse(frac);
    }
    return int.parse(cleaned) * 100;
  }

  Future<void> _capture() async {
    final amountMinor = _parseMinor(_amount.text);
    final key = 'expense-${widget.idGenerator()}';
    final capture = ExpenseCaptureService(widget.queue);
    capture.captureOffline(
      amountMinor: amountMinor,
      currency: _currency.text.trim().toUpperCase(),
      description: _desc.text.trim().isEmpty ? 'Receipt' : _desc.text.trim(),
      receiptUrl: 'camera://local/${widget.idGenerator()}',
      idempotencyKey: key,
    );

    if (_forceOffline) {
      setState(() => _message = 'Offline — queued with key $key');
      return;
    }

    final result = await widget.queue.replay((m) async {
      final res = await widget.api.post(
        m.path,
        body: m.body,
        idempotencyKey: m.idempotencyKey,
      );
      return TransportResult(statusCode: res.statusCode, body: res.body);
    });
    setState(() {
      _message = result.replayed > 0
          ? 'Submitted (key $key)'
          : 'Queued for retry (key $key)';
    });
  }

  @override
  Widget build(BuildContext context) {
    return ListView(
      padding: const EdgeInsets.all(20),
      children: [
        Text('Create expense', style: Theme.of(context).textTheme.headlineSmall),
        const SizedBox(height: 8),
        const Text('Camera-first capture. Amounts are integer minor units.'),
        const SizedBox(height: 16),
        Container(
          height: 180,
          alignment: Alignment.center,
          decoration: BoxDecoration(
            gradient: const LinearGradient(
              colors: [Color(0xFF0F6E56), Color(0xFF147D64)],
              begin: Alignment.topLeft,
              end: Alignment.bottomRight,
            ),
            borderRadius: BorderRadius.circular(12),
          ),
          child: const Column(
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              Icon(Icons.photo_camera, size: 48, color: Colors.white),
              SizedBox(height: 8),
              Text('Tap capture to attach receipt',
                  style: TextStyle(color: Colors.white)),
            ],
          ),
        ),
        const SizedBox(height: 16),
        TextField(
            controller: _desc,
            decoration: const InputDecoration(labelText: 'Description')),
        TextField(
          controller: _amount,
          decoration:
              const InputDecoration(labelText: 'Amount (major units)'),
          keyboardType:
              const TextInputType.numberWithOptions(decimal: true),
        ),
        TextField(
            controller: _currency,
            decoration: const InputDecoration(labelText: 'Currency')),
        SwitchListTile(
          title: const Text('Simulate offline'),
          value: _forceOffline,
          onChanged: (v) => setState(() => _forceOffline = v),
        ),
        FilledButton.icon(
          onPressed: _capture,
          icon: const Icon(Icons.camera_alt),
          label: const Text('Capture & submit'),
        ),
        if (_message != null) ...[
          const SizedBox(height: 12),
          Text(_message!),
        ],
        Text('Queued: ${widget.queue.items.length}'),
      ],
    );
  }
}
