'use client';

import { useCallback, useEffect, useState } from 'react';
import Link from 'next/link';
import {
  Button,
  EmptyState,
  ErrorState,
  LoadingState,
  PermissionDeniedState,
  Table,
} from '@companyos/design-system';
import {
  MarketplaceNav,
  MarketplacePage,
  ScopeBadges,
  StatusBadge,
} from '../../../components/MarketplacePage';
import { authFetch, getAccessToken } from '../../../lib/auth-client';
import { useCapabilities } from '../../../lib/capabilities';
import {
  itemsFrom,
  responseMessage,
  type MarketplaceInstall,
} from '../../../lib/marketplace';

function installName(install: MarketplaceInstall): string {
  return install.listing?.name ?? install.app_name ?? install.name ?? 'Marketplace app';
}

export default function MarketplaceInstallsPage() {
  const { can, loading: capabilitiesLoading } = useCapabilities();
  const [installs, setInstalls] = useState<MarketplaceInstall[]>([]);
  const [loading, setLoading] = useState(true);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const canRead = can('admin.marketplace.read');
  const canUninstall = can('admin.marketplace.uninstall');

  const loadInstalls = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const response = await authFetch('/api/v1/marketplace/installs');
      if (!response.ok) {
        setError(await responseMessage(response, 'Could not load installed apps.'));
        return;
      }
      const body = await response.json();
      setInstalls(itemsFrom<MarketplaceInstall>(body, ['installs']));
    } catch {
      setError('The installed apps request failed.');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (!capabilitiesLoading && canRead) void loadInstalls();
    if (!capabilitiesLoading && !canRead) setLoading(false);
  }, [capabilitiesLoading, canRead, loadInstalls]);

  async function uninstall(install: MarketplaceInstall) {
    if (!canUninstall) return;
    setBusyId(install.id);
    setError(null);
    try {
      const response = await authFetch(
        `/api/v1/marketplace/installs/${encodeURIComponent(install.id)}`,
        { method: 'DELETE' },
      );
      if (!response.ok) {
        setError(await responseMessage(response, `Could not uninstall ${installName(install)}.`));
        return;
      }
      await loadInstalls();
    } catch {
      setError('The uninstall request failed.');
    } finally {
      setBusyId(null);
    }
  }

  if (!getAccessToken()) {
    return <ErrorState title="Sign in required" message="Sign in to manage installed apps." />;
  }
  if (capabilitiesLoading) return <LoadingState label="Loading marketplace permissions" />;
  if (!canRead) return <PermissionDeniedState requiredPermission="admin.marketplace.read" />;

  return (
    <MarketplacePage
      title="Installed apps"
      description="Review marketplace access granted to this organization and remove apps no longer in use."
    >
      <MarketplaceNav
        canWrite={can('admin.marketplace.write')}
        canReview={can('admin.marketplace.review')}
      />
      {error ? <ErrorState message={error} /> : null}
      {!error && loading ? <LoadingState label="Loading installed apps" /> : null}
      {!loading ? (
        <Table
          columns={[
            {
              key: 'name',
              header: 'App',
              cell: (install: MarketplaceInstall) => {
                const listingId = install.listing?.id ?? install.listing_id ?? install.app_id;
                return listingId ? (
                  <Link
                    href={`/marketplace/${encodeURIComponent(listingId)}`}
                    style={{ color: 'var(--cos-color-accent)', fontWeight: 600 }}
                  >
                    {installName(install)}
                  </Link>
                ) : (
                  installName(install)
                );
              },
            },
            {
              key: 'scopes',
              header: 'Consented scopes',
              minWidth: 260,
              cell: (install: MarketplaceInstall) => (
                <ScopeBadges scopes={install.consented_scopes ?? install.scopes ?? []} />
              ),
            },
            {
              key: 'status',
              header: 'Status',
              cell: (install: MarketplaceInstall) => <StatusBadge status={install.status} />,
            },
            {
              key: 'actions',
              header: '',
              align: 'right',
              cell: (install: MarketplaceInstall) =>
                canUninstall && install.status.toLowerCase() !== 'uninstalled' ? (
                  <Button
                    variant="danger"
                    size="sm"
                    loading={busyId === install.id}
                    disabled={busyId !== null}
                    onClick={() => void uninstall(install)}
                  >
                    Uninstall
                  </Button>
                ) : (
                  '—'
                ),
            },
          ]}
          rows={installs}
          getRowKey={(install) => install.id}
          empty={
            <EmptyState
              title="No installed apps"
              description="Install an app from the marketplace catalogue to see it here."
            />
          }
        />
      ) : null}
    </MarketplacePage>
  );
}
