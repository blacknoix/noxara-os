import 'dart:convert';

/// Offline mutation queue — mirrors web `apps/web/lib/offline/queue.ts`.
///
/// Replays MUST reuse the same [idempotencyKey] so POST /expenses cannot duplicate.
class QueuedMutation {
  QueuedMutation({
    required this.id,
    required this.createdAt,
    required this.method,
    required this.path,
    required this.idempotencyKey,
    required this.label,
    this.body,
    this.ifMatch,
  });

  final String id;
  final int createdAt;
  final String method; // POST | PATCH | DELETE
  final String path;
  final Map<String, dynamic>? body;
  final String idempotencyKey;
  final int? ifMatch;
  final String label;

  Map<String, dynamic> toJson() => {
        'id': id,
        'createdAt': createdAt,
        'method': method,
        'path': path,
        'body': body,
        'idempotencyKey': idempotencyKey,
        'ifMatch': ifMatch,
        'label': label,
      };

  factory QueuedMutation.fromJson(Map<String, dynamic> json) {
    return QueuedMutation(
      id: json['id'] as String,
      createdAt: json['createdAt'] as int,
      method: json['method'] as String,
      path: json['path'] as String,
      body: json['body'] as Map<String, dynamic>?,
      idempotencyKey: json['idempotencyKey'] as String,
      ifMatch: json['ifMatch'] as int?,
      label: json['label'] as String,
    );
  }
}

typedef MutationTransport = Future<TransportResult> Function(
  QueuedMutation mutation,
);

class TransportResult {
  TransportResult({required this.statusCode, this.body = ''});
  final int statusCode;
  final String body;
  bool get ok => statusCode >= 200 && statusCode < 300;
}

class ReplayResult {
  ReplayResult({
    required this.replayed,
    required this.remaining,
    required this.seenIdempotencyKeys,
  });

  final int replayed;
  final List<QueuedMutation> remaining;
  /// Every attempt's Idempotency-Key — tests assert stability across loss/replay.
  final List<String> seenIdempotencyKeys;
}

/// In-memory + optional string-store backed queue.
class MutationQueue {
  MutationQueue({this.idGenerator});

  final String Function()? idGenerator;
  final List<QueuedMutation> _queue = [];

  List<QueuedMutation> get items => List.unmodifiable(_queue);

  void clear() => _queue.clear();

  QueuedMutation enqueue({
    required String method,
    required String path,
    required String idempotencyKey,
    required String label,
    Map<String, dynamic>? body,
    int? ifMatch,
    String? id,
    int? createdAt,
  }) {
    final item = QueuedMutation(
      id: id ?? (idGenerator?.call() ?? 'q-${DateTime.now().microsecondsSinceEpoch}'),
      createdAt: createdAt ?? DateTime.now().millisecondsSinceEpoch,
      method: method,
      path: path,
      body: body,
      idempotencyKey: idempotencyKey,
      ifMatch: ifMatch,
      label: label,
    );
    _queue.add(item);
    return item;
  }

  String serialize() => jsonEncode(_queue.map((e) => e.toJson()).toList());

  void loadFrom(String? raw) {
    _queue.clear();
    if (raw == null || raw.isEmpty) return;
    final list = jsonDecode(raw) as List<dynamic>;
    for (final item in list) {
      _queue.add(QueuedMutation.fromJson(item as Map<String, dynamic>));
    }
  }

  /// Replay FIFO. Same Idempotency-Key on every attempt.
  ///
  /// When [forceOffline] is true, the first N items fail as network errors and
  /// stay queued (simulates connectivity loss without dropping the key).
  Future<ReplayResult> replay(
    MutationTransport transport, {
    int forceOfflineCount = 0,
  }) async {
    final remaining = <QueuedMutation>[];
    final seenKeys = <String>[];
    var replayed = 0;
    var offlineLeft = forceOfflineCount;

    for (final m in List<QueuedMutation>.from(_queue)) {
      seenKeys.add(m.idempotencyKey);
      if (offlineLeft > 0) {
        offlineLeft -= 1;
        remaining.add(m);
        continue;
      }
      try {
        final res = await transport(m);
        if (res.ok || res.statusCode == 201) {
          replayed += 1;
          continue;
        }
        // Keep non-success for retry (except conflict — drop like web).
        if (res.statusCode == 409 || res.statusCode == 412) {
          continue;
        }
        remaining.add(m);
      } catch (_) {
        remaining.add(m);
      }
    }

    _queue
      ..clear()
      ..addAll(remaining);
    return ReplayResult(
      replayed: replayed,
      remaining: List.unmodifiable(remaining),
      seenIdempotencyKeys: seenKeys,
    );
  }
}

/// Captures an expense while offline; queues with a stable idempotency key.
class ExpenseCaptureService {
  ExpenseCaptureService(this.queue);

  final MutationQueue queue;

  /// Returns the idempotency key used (stable across retries).
  String captureOffline({
    required int amountMinor,
    required String currency,
    required String description,
    String? receiptUrl,
    String? categoryCode,
    String? idempotencyKey,
  }) {
    assert(amountMinor > 0, 'amount_minor must be positive integer minor units');
    final key = idempotencyKey ??
        'expense-${DateTime.now().millisecondsSinceEpoch}-$amountMinor';
    queue.enqueue(
      method: 'POST',
      path: '/api/v1/finance/expenses',
      idempotencyKey: key,
      label: 'Submit expense',
      body: {
        'amount_minor': amountMinor,
        'currency': currency,
        'description': description,
        if (receiptUrl != null) 'receipt_url': receiptUrl,
        if (categoryCode != null) 'category_code': categoryCode,
      },
    );
    return key;
  }
}
