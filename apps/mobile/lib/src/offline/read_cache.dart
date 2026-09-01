import 'dart:convert';

/// Simple path → JSON read cache for offline dashboard / lists.
class ReadCache {
  final Map<String, CachedEntry> _entries = {};

  void put(String path, Object? data) {
    _entries[path] = CachedEntry(
      asOf: DateTime.now().millisecondsSinceEpoch,
      data: data,
    );
  }

  CachedEntry? get(String path) => _entries[path];

  String serialize() {
    final map = <String, dynamic>{};
    for (final e in _entries.entries) {
      map[e.key] = {'asOf': e.value.asOf, 'data': e.value.data};
    }
    return jsonEncode(map);
  }

  void loadFrom(String? raw) {
    _entries.clear();
    if (raw == null || raw.isEmpty) return;
    final map = jsonDecode(raw) as Map<String, dynamic>;
    for (final e in map.entries) {
      final v = e.value as Map<String, dynamic>;
      _entries[e.key] = CachedEntry(
        asOf: v['asOf'] as int,
        data: v['data'],
      );
    }
  }

  void clear() => _entries.clear();
}

class CachedEntry {
  CachedEntry({required this.asOf, required this.data});
  final int asOf;
  final Object? data;
}
