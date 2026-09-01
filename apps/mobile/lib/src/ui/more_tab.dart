import 'package:flutter/material.dart';

import '../auth/auth_service.dart';
import '../biometrics/biometric_service.dart';

class MoreTab extends StatefulWidget {
  const MoreTab({
    super.key,
    required this.auth,
    required this.biometrics,
    this.onDeepLink,
  });

  final AuthService auth;
  final BiometricService biometrics;
  final Future<void> Function(String uri)? onDeepLink;

  @override
  State<MoreTab> createState() => _MoreTabState();
}

class _MoreTabState extends State<MoreTab> {
  List<Membership> _memberships = [];
  String? _bioStatus;

  @override
  void initState() {
    super.initState();
    _loadMemberships();
  }

  Future<void> _loadMemberships() async {
    try {
      final items = await widget.auth.listMemberships();
      if (mounted) setState(() => _memberships = items);
    } catch (_) {
      // offline / unauthenticated — ignore
    }
  }

  Future<void> _switch(String orgId) async {
    await widget.auth.switchOrg(orgId);
    if (mounted) setState(() {});
  }

  Future<void> _bio() async {
    final ok = await widget.biometrics.authenticate(reason: 'Confirm identity');
    setState(() => _bioStatus = ok ? 'unlocked' : 'failed');
  }

  @override
  Widget build(BuildContext context) {
    return ListView(
      padding: const EdgeInsets.all(20),
      children: [
        Text('More', style: Theme.of(context).textTheme.headlineSmall),
        const SizedBox(height: 16),
        Text('Organization', style: Theme.of(context).textTheme.titleMedium),
        Text('Current: ${widget.auth.orgId ?? "—"}'),
        ..._memberships.map(
          (m) => ListTile(
            title: Text(m.orgName),
            subtitle: Text('${m.orgId} · ${m.role}'),
            trailing: m.orgId == widget.auth.orgId
                ? const Icon(Icons.check, color: Color(0xFF0F6E56))
                : TextButton(onPressed: () => _switch(m.orgId), child: const Text('Switch')),
          ),
        ),
        const Divider(),
        ListTile(
          leading: const Icon(Icons.fingerprint),
          title: const Text('Biometric unlock'),
          subtitle: Text(_bioStatus ?? 'Fake biometric in CI'),
          onTap: _bio,
        ),
        ListTile(
          leading: const Icon(Icons.link),
          title: const Text('Open sample deep link'),
          onTap: () => widget.onDeepLink?.call(
            'companyos://record/exp_demo?org=${widget.auth.orgId ?? "org_demo"}',
          ),
        ),
        ListTile(
          leading: const Icon(Icons.logout),
          title: const Text('Sign out'),
          onTap: () {
            widget.auth.signOut();
          },
        ),
      ],
    );
  }
}
