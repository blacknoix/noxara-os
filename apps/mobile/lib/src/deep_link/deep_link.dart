/// Deep link parsing for `companyos://record/{id}`.
///
/// Org routing: optional `org` query (or path segment) selects the workspace
/// before navigation. Push payloads should include org_id; the bare URI form
/// is still accepted and uses the current session org when org is omitted.
class DeepLink {
  DeepLink({
    required this.recordId,
    this.orgId,
    this.kind,
  });

  /// Public record id (`exp_…`, `dl_…`, `tsk_…`, `apr_…`, …).
  final String recordId;

  /// Target organization public id (`org_…`) when known.
  final String? orgId;

  /// Derived from [recordId] prefix when recognizable.
  final String? kind;

  static final _recordUri = RegExp(
    r'^companyos://(?:org/([^/]+)/)?record/([^/?#]+)(?:\?(.*))?$',
    caseSensitive: false,
  );

  /// Parse a companyos deep link. Returns null if the URI is not ours.
  static DeepLink? parse(String uri) {
    final trimmed = uri.trim();
    final match = _recordUri.firstMatch(trimmed);
    if (match == null) return null;

    final pathOrg = match.group(1);
    final recordId = match.group(2)!;
    final query = match.group(3);
    String? orgId = pathOrg;
    if (query != null && query.isNotEmpty) {
      final params = Uri.splitQueryString(query);
      orgId ??= params['org'] ?? params['org_id'];
    }

    return DeepLink(
      recordId: recordId,
      orgId: orgId,
      kind: kindFromRecordId(recordId),
    );
  }

  static String? kindFromRecordId(String recordId) {
    const prefixes = {
      'exp_': 'expense',
      'dl_': 'deal',
      'tsk_': 'task',
      'apr_': 'approval',
      'inv_': 'invoice',
      'cus_': 'customer',
      'prj_': 'project',
      'ntf_': 'notification',
    };
    for (final e in prefixes.entries) {
      if (recordId.startsWith(e.key)) return e.value;
    }
    return null;
  }

  /// Build a canonical URI (includes org when known).
  String toUri() {
    if (orgId != null && orgId!.isNotEmpty) {
      return 'companyos://record/$recordId?org=$orgId';
    }
    return 'companyos://record/$recordId';
  }
}

/// Result of opening a deep link in the correct organization.
class DeepLinkNavigation {
  DeepLinkNavigation({
    required this.recordId,
    required this.orgId,
    required this.switchedOrg,
    required this.route,
  });

  final String recordId;
  final String orgId;
  final bool switchedOrg;
  final String route;
}

typedef SwitchOrgFn = Future<void> Function(String orgId);

/// Opens a deep link: switch org when needed, then resolve in-app route.
class DeepLinkRouter {
  DeepLinkRouter({
    required this.currentOrgId,
    required this.switchOrg,
  });

  final String? Function() currentOrgId;
  final SwitchOrgFn switchOrg;

  Future<DeepLinkNavigation> open(DeepLink link) async {
    final current = currentOrgId();
    final targetOrg = link.orgId ?? current;
    if (targetOrg == null || targetOrg.isEmpty) {
      throw StateError('deep link requires org context');
    }

    var switched = false;
    if (current != targetOrg) {
      await switchOrg(targetOrg);
      switched = true;
    }

    return DeepLinkNavigation(
      recordId: link.recordId,
      orgId: targetOrg,
      switchedOrg: switched,
      route: RecordRoutes.pathFor(link.recordId),
    );
  }
}

/// Maps public ids to in-app routes (high-frequency 1.11 surface).
class RecordRoutes {
  static String pathFor(String recordId) {
    final kind = DeepLink.kindFromRecordId(recordId);
    switch (kind) {
      case 'expense':
        return '/finance/expenses/$recordId';
      case 'deal':
        return '/sales/deals/$recordId';
      case 'task':
        return '/ops/tasks/$recordId';
      case 'approval':
        return '/approvals/$recordId';
      case 'invoice':
        return '/finance/invoices/$recordId';
      default:
        return '/record/$recordId';
    }
  }
}
