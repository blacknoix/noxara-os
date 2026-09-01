import 'package:flutter_test/flutter_test.dart';
import 'package:companyos_mobile/src/offline/mutation_queue.dart';

void main() {
  group('offline expense capture idempotency', () {
    test('queues and replays with the same idempotency key — no duplicate', () async {
      final queue = MutationQueue(idGenerator: () => 'm1');
      const key = 'idem-expense-1';
      final capture = ExpenseCaptureService(queue);

      capture.captureOffline(
        amountMinor: 1000,
        currency: 'USD',
        description: 'Taxi',
        idempotencyKey: key,
      );
      expect(queue.items, hasLength(1));
      expect(queue.items.first.idempotencyKey, key);
      expect(queue.items.first.body!['amount_minor'], 1000);

      // Forced connectivity loss: mutation stays queued with same key.
      final offline = await queue.replay(
        (m) async => TransportResult(statusCode: 201),
        forceOfflineCount: 1,
      );
      expect(offline.replayed, 0);
      expect(offline.remaining, hasLength(1));
      expect(offline.seenIdempotencyKeys, [key]);
      expect(queue.items.single.idempotencyKey, key);

      // Server sees one logical create; tracks POST attempts by key.
      final createdIds = <String>[];
      final seenKeys = <String>[];
      final online = await queue.replay((m) async {
        seenKeys.add(m.idempotencyKey);
        if (createdIds.isEmpty) {
          createdIds.add('exp_1');
          return TransportResult(statusCode: 201, body: '{"id":"exp_1"}');
        }
        // Idempotent replay — same resource, not a duplicate create.
        return TransportResult(statusCode: 200, body: '{"id":"exp_1"}');
      });
      expect(online.replayed, 1);
      expect(seenKeys, [key]);
      expect(createdIds, ['exp_1']);
      expect(queue.items, isEmpty);

      // Second logical enqueue with same key still sends that key.
      capture.captureOffline(
        amountMinor: 1000,
        currency: 'USD',
        description: 'Taxi',
        idempotencyKey: key,
      );
      await queue.replay((m) async {
        seenKeys.add(m.idempotencyKey);
        return TransportResult(statusCode: 200, body: '{"id":"exp_1"}');
      });
      expect(seenKeys, [key, key]);
      expect(createdIds, ['exp_1']); // still one expense id
    });

    test('amount_minor stays integer — no float money path', () {
      final queue = MutationQueue();
      ExpenseCaptureService(queue).captureOffline(
        amountMinor: 1299,
        currency: 'EUR',
        description: 'Lunch',
        idempotencyKey: 'k2',
      );
      expect(queue.items.single.body!['amount_minor'], isA<int>());
      expect(queue.items.single.body!['amount_minor'], 1299);
    });
  });
}
