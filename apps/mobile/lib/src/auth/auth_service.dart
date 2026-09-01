import 'dart:convert';

import '../api/api_client.dart';

class AuthSession {
  AuthSession({
    required this.accessToken,
    required this.orgId,
    required this.sessionId,
    this.userId,
    this.roles = const [],
  });

  final String accessToken;
  final String orgId;
  final String sessionId;
  final String? userId;
  final List<String> roles;
}

class Membership {
  Membership({
    required this.orgId,
    required this.orgName,
    required this.role,
    required this.policyVersion,
  });

  final String orgId;
  final String orgName;
  final String role;
  final int policyVersion;

  factory Membership.fromJson(Map<String, dynamic> json) {
    return Membership(
      orgId: json['org_id'] as String,
      orgName: json['org_name'] as String? ?? json['org_id'] as String,
      role: json['role'] as String? ?? '',
      policyVersion: (json['policy_version'] as num?)?.toInt() ?? 0,
    );
  }
}

/// Auth against existing CompanyOS `/api/v1/auth/*` (LOCAL_AUTH in CI).
class AuthService {
  AuthService(this.api);

  final ApiClient api;
  AuthSession? _session;

  AuthSession? get session => _session;
  String? get accessToken => _session?.accessToken;
  String? get orgId => _session?.orgId;

  Future<AuthSession> login({
    required String email,
    required String password,
    String? orgId,
  }) async {
    final res = await api.post('/api/v1/auth/login', body: {
      'email': email,
      'password': password,
      if (orgId != null) 'org_id': orgId,
      'device_label': 'companyos-mobile',
    });
    if (res.statusCode == 200) {
      final body = jsonDecode(res.body) as Map<String, dynamic>;
      if (body['mfa_required'] == true) {
        throw AuthException('mfa_required', challengeToken: body['challenge_token'] as String?);
      }
      return _applyTokenResponse(body);
    }
    throw AuthException('login_failed:${res.statusCode}');
  }

  Future<AuthSession> switchOrg(String targetOrgId) async {
    final res = await api.post('/api/v1/auth/switch-org', body: {
      'org_id': targetOrgId,
    });
    if (res.statusCode != 200) {
      throw AuthException('switch_org_failed:${res.statusCode}');
    }
    final body = jsonDecode(res.body) as Map<String, dynamic>;
    return _applyTokenResponse(body, orgIdOverride: targetOrgId);
  }

  Future<List<Membership>> listMemberships() async {
    final res = await api.get('/api/v1/auth/memberships');
    if (res.statusCode != 200) {
      throw AuthException('memberships_failed:${res.statusCode}');
    }
    final body = jsonDecode(res.body) as Map<String, dynamic>;
    final items = (body['items'] as List<dynamic>? ?? [])
        .cast<Map<String, dynamic>>()
        .map(Membership.fromJson)
        .toList();
    return items;
  }

  AuthSession _applyTokenResponse(
    Map<String, dynamic> body, {
    String? orgIdOverride,
  }) {
    final token = body['access_token'] as String;
    final sessionId = body['session_id'] as String? ?? '';
    // Prefer explicit override (switch-org); otherwise decode JWT payload org_id.
    final orgId = orgIdOverride ?? _orgIdFromJwt(token) ?? '';
    _session = AuthSession(
      accessToken: token,
      orgId: orgId,
      sessionId: sessionId,
    );
    return _session!;
  }

  static String? _orgIdFromJwt(String token) {
    final parts = token.split('.');
    if (parts.length < 2) return null;
    try {
      final normalized = base64Url.normalize(parts[1]);
      final payload =
          jsonDecode(utf8.decode(base64Url.decode(normalized))) as Map<String, dynamic>;
      return payload['org_id'] as String?;
    } catch (_) {
      return null;
    }
  }

  void signOut() {
    _session = null;
  }
}

class AuthException implements Exception {
  AuthException(this.code, {this.challengeToken});
  final String code;
  final String? challengeToken;

  @override
  String toString() => 'AuthException($code)';
}
