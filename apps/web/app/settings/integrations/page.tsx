'use client';

import { useCallback, useEffect, useState } from 'react';
import Link from 'next/link';
import {
  Button,
  Card,
  Checkbox,
  ErrorState,
  InlineAlert,
  LoadingState,
  PermissionDeniedState,
} from '@companyos/design-system';
import { ScopeBadges, StatusBadge, marketplaceStyles } from '../../../components/MarketplacePage';
import { authFetch, getAccessToken } from '../../../lib/auth-client';
import { useCapabilities } from '../../../lib/capabilities';
import { humanize, itemsFrom, responseMessage, type Integration } from '../../../lib/marketplace';

const primaryConnectors: Integration[] = [
  {
    connector_key: 'email.google',
    name: 'Google email',
    description: 'Connect Gmail for organization email workflows.',
    status: 'disconnected',
    available_scopes: ['email.read', 'email.send'],
  },
  {
    connector_key: 'calendar.microsoft',
    name: 'Microsoft calendar',
    description: 'Connect Microsoft 365 calendars for scheduling.',
    status: 'disconnected',
    available_scopes: ['calendar.read', 'calendar.write'],
  },
  {
    connector_key: 'payments.stripe',
    name: 'Stripe payments',
    description: 'Connect Stripe to synchronize payment activity.',
    status: 'disconnected',
    available_scopes: ['finance.payment.read', 'finance.payment.create'],
  },
];

function availableScopes(integration: Integration): string[] {
  return integration.available_scopes ?? integration.requested_scopes ?? integration.scopes ?? [];
}

function connected(integration: Integration): boolean {
  return ['connected', 'active', 'installed'].includes(integration.status.toLowerCase());
}

export default function IntegrationsSettingsPage() {
  const { can, loading: capabilitiesLoading } = useCapabilities();
  const [integrations, setIntegrations] = useState<Integration[]>([]);
  const [selectedScopes, setSelectedScopes] = useState<Record<string, string[]>>({});
  const [loading, setLoading] = useState(true);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const canRead = can('admin.marketplace.read');
  const canConnect = can('admin.marketplace.install');
  const canDisconnect = can('admin.marketplace.uninstall');

  const loadIntegrations = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const response = await authFetch('/api/v1/integrations');
      if (!response.ok) {
        setError(await responseMessage(response, 'Could not load integrations.'));
        return;
      }
      const body = await response.json();
      const returned = itemsFrom<Integration>(body, ['integrations', 'connectors']);
      const byKey = new Map(
        returned.map((integration) => [integration.connector_key, integration]),
      );
      const merged = primaryConnectors.map((primary) => ({
        ...primary,
        ...byKey.get(primary.connector_key),
      }));
      const primaryKeys = new Set(
        primaryConnectors.map((integration) => integration.connector_key),
      );
      merged.push(...returned.filter((integration) => !primaryKeys.has(integration.connector_key)));
      setIntegrations(merged);
      setSelectedScopes((current) => ({
        ...Object.fromEntries(
          merged.map((integration) => [
            integration.connector_key,
            integration.scopes ?? availableScopes(integration),
          ]),
        ),
        ...current,
      }));
    } catch {
      setError('The integrations request failed.');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (!capabilitiesLoading && canRead) void loadIntegrations();
    if (!capabilitiesLoading && !canRead) setLoading(false);
  }, [capabilitiesLoading, canRead, loadIntegrations]);

  function toggleScope(connectorKey: string, scope: string) {
    setSelectedScopes((current) => {
      const selected = current[connectorKey] ?? [];
      return {
        ...current,
        [connectorKey]: selected.includes(scope)
          ? selected.filter((item) => item !== scope)
          : [...selected, scope],
      };
    });
  }

  async function connect(integration: Integration) {
    if (!canConnect) return;
    setBusyKey(integration.connector_key);
    setError(null);
    try {
      const response = await authFetch(
        `/api/v1/integrations/${encodeURIComponent(integration.connector_key)}/connect`,
        {
          method: 'POST',
          body: JSON.stringify({
            consented_scopes: selectedScopes[integration.connector_key] ?? [],
          }),
        },
      );
      if (!response.ok) {
        setError(await responseMessage(response, `Could not connect ${integration.name}.`));
        return;
      }
      await loadIntegrations();
    } catch {
      setError('The integration connect request failed.');
    } finally {
      setBusyKey(null);
    }
  }

  async function disconnect(integration: Integration) {
    if (!canDisconnect) return;
    setBusyKey(integration.connector_key);
    setError(null);
    try {
      const response = await authFetch(
        `/api/v1/integrations/${encodeURIComponent(integration.connector_key)}/disconnect`,
        { method: 'POST' },
      );
      if (!response.ok) {
        setError(await responseMessage(response, `Could not disconnect ${integration.name}.`));
        return;
      }
      await loadIntegrations();
    } catch {
      setError('The integration disconnect request failed.');
    } finally {
      setBusyKey(null);
    }
  }

  if (!getAccessToken()) {
    return <ErrorState title="Sign in required" message="Sign in to manage integrations." />;
  }
  if (capabilitiesLoading) return <LoadingState label="Loading integration permissions" />;
  if (!canRead) return <PermissionDeniedState requiredPermission="admin.marketplace.read" />;

  return (
    <div style={{ display: 'grid', gap: 'var(--cos-space-5)', maxWidth: 1040, margin: '0 auto' }}>
      <header style={marketplaceStyles.cardHeader}>
        <div>
          <p
            style={{
              margin: 0,
              textTransform: 'uppercase',
              letterSpacing: '0.08em',
              fontSize: '0.72rem',
              color: 'var(--cos-color-fg-muted)',
              fontWeight: 600,
            }}
          >
            Settings
          </p>
          <h1
            style={{
              margin: '0.35rem 0 0',
              fontFamily: 'var(--cos-font-display)',
              fontSize: '1.75rem',
              fontWeight: 650,
            }}
          >
            Integrations
          </h1>
          <p style={marketplaceStyles.muted}>
            Connect first-party services through the same scoped marketplace install model.
          </p>
        </div>
        <Link href="/marketplace/installs" style={marketplaceStyles.linkButton}>
          View installed apps
        </Link>
      </header>

      {error ? <ErrorState message={error} /> : null}
      {!error && loading ? <LoadingState label="Loading integrations" /> : null}
      {!error && !loading ? (
        <div style={marketplaceStyles.cardGrid}>
          {integrations.map((integration) => {
            const isConnected = connected(integration);
            const scopes = availableScopes(integration);
            return (
              <Card key={integration.connector_key} as="article" style={marketplaceStyles.section}>
                <div style={marketplaceStyles.cardHeader}>
                  <div>
                    <h2 style={{ margin: 0, fontSize: '1.1rem' }}>
                      {integration.name ?? humanize(integration.connector_key)}
                    </h2>
                    <p style={marketplaceStyles.muted}>
                      {integration.description ?? integration.connector_key}
                    </p>
                  </div>
                  <StatusBadge status={integration.status} />
                </div>

                {integration.last_error ? (
                  <InlineAlert tone="danger" title="Last connection error">
                    {integration.last_error}
                  </InlineAlert>
                ) : null}

                {isConnected ? (
                  <div>
                    <strong style={{ fontSize: '0.8125rem' }}>Granted scopes</strong>
                    <div style={{ marginTop: 'var(--cos-space-2)' }}>
                      <ScopeBadges scopes={integration.scopes ?? scopes} />
                    </div>
                  </div>
                ) : (
                  <fieldset style={marketplaceStyles.fieldset}>
                    <legend style={marketplaceStyles.legend}>Requested scopes</legend>
                    {scopes.length === 0 ? (
                      <span style={marketplaceStyles.muted}>
                        This connector does not request additional scopes.
                      </span>
                    ) : (
                      scopes.map((scope) => (
                        <Checkbox
                          key={scope}
                          label={scope}
                          checked={(selectedScopes[integration.connector_key] ?? []).includes(
                            scope,
                          )}
                          disabled={!canConnect || busyKey !== null}
                          onChange={() => toggleScope(integration.connector_key, scope)}
                        />
                      ))
                    )}
                  </fieldset>
                )}

                {isConnected ? (
                  <Button
                    variant="danger"
                    loading={busyKey === integration.connector_key}
                    disabled={!canDisconnect || busyKey !== null}
                    onClick={() => void disconnect(integration)}
                  >
                    Disconnect
                  </Button>
                ) : (
                  <Button
                    loading={busyKey === integration.connector_key}
                    disabled={!canConnect || busyKey !== null}
                    onClick={() => void connect(integration)}
                  >
                    Connect
                  </Button>
                )}
              </Card>
            );
          })}
        </div>
      ) : null}
    </div>
  );
}
