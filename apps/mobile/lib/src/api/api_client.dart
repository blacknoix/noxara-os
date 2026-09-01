import 'dart:convert';

import 'package:http/http.dart' as http;

/// HTTP client that always attaches Bearer access token.
///
/// Org isolation is via the JWT `org_id` claim (switch-org mints a new token).
/// Never swap org via a client header.
class ApiClient {
  ApiClient({
    required this.baseUrl,
    http.Client? httpClient,
    this.getAccessToken,
  }) : _http = httpClient ?? http.Client();

  final String baseUrl;
  final http.Client _http;
  final String? Function()? getAccessToken;

  Uri _uri(String path) {
    final base = baseUrl.endsWith('/')
        ? baseUrl.substring(0, baseUrl.length - 1)
        : baseUrl;
    return Uri.parse('$base$path');
  }

  Map<String, String> _headers({
    String? idempotencyKey,
    int? ifMatch,
    Map<String, String>? extra,
  }) {
    final headers = <String, String>{
      'Content-Type': 'application/json',
      'Accept': 'application/json',
      ...?extra,
    };
    final token = getAccessToken?.call();
    if (token != null && token.isNotEmpty) {
      headers['Authorization'] = 'Bearer $token';
    }
    if (idempotencyKey != null) {
      headers['Idempotency-Key'] = idempotencyKey;
    }
    if (ifMatch != null) {
      headers['If-Match'] = '$ifMatch';
    }
    return headers;
  }

  Future<http.Response> get(String path) {
    return _http.get(_uri(path), headers: _headers());
  }

  Future<http.Response> post(
    String path, {
    Object? body,
    String? idempotencyKey,
  }) {
    return _http.post(
      _uri(path),
      headers: _headers(idempotencyKey: idempotencyKey),
      body: body == null ? null : jsonEncode(body),
    );
  }

  Future<http.Response> patch(
    String path, {
    Object? body,
    String? idempotencyKey,
    int? ifMatch,
  }) {
    return _http.patch(
      _uri(path),
      headers: _headers(idempotencyKey: idempotencyKey, ifMatch: ifMatch),
      body: body == null ? null : jsonEncode(body),
    );
  }

  Future<http.Response> delete(String path) {
    return _http.delete(_uri(path), headers: _headers());
  }
}

/// Thrown when the transport layer fails (forced offline in tests).
class NetworkUnavailable implements Exception {
  NetworkUnavailable([this.message = 'network unavailable']);
  final String message;

  @override
  String toString() => 'NetworkUnavailable($message)';
}
