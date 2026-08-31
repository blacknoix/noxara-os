'use client';

import { useCallback, useEffect, useState } from 'react';
import Link from 'next/link';
import {
  Badge,
  EmptyState,
  ErrorState,
  LoadingState,
  PermissionDeniedState,
  Table,
} from '@companyos/design-system';
import {
  MarketplaceNav,
  MarketplacePage,
  marketplaceStyles,
} from '../../components/MarketplacePage';
import { authFetch, getAccessToken } from '../../lib/auth-client';
import { useCapabilities } from '../../lib/capabilities';
import { itemsFrom, responseMessage, type MarketplaceListing } from '../../lib/marketplace';

export default function MarketplaceCataloguePage() {
  const { can, loading: capabilitiesLoading } = useCapabilities();
  const [listings, setListings] = useState<MarketplaceListing[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const canRead = can('admin.marketplace.read');

  const loadCatalogue = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const response = await authFetch('/api/v1/marketplace/catalogue');
      if (!response.ok) {
        setError(await responseMessage(response, 'Could not load the marketplace catalogue.'));
        return;
      }
      const body = await response.json();
      setListings(itemsFrom<MarketplaceListing>(body, ['listings', 'catalogue']));
    } catch {
      setError('The marketplace catalogue request failed.');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (!capabilitiesLoading && canRead) void loadCatalogue();
    if (!capabilitiesLoading && !canRead) setLoading(false);
  }, [capabilitiesLoading, canRead, loadCatalogue]);

  if (!getAccessToken()) {
    return <ErrorState title="Sign in required" message="Sign in to browse the marketplace." />;
  }
  if (capabilitiesLoading) return <LoadingState label="Loading marketplace permissions" />;
  if (!canRead) return <PermissionDeniedState requiredPermission="admin.marketplace.read" />;

  return (
    <MarketplacePage
      title="App marketplace"
      description="Browse approved apps and review the access each app requests before installing it."
      actions={
        <Link href="/marketplace/installs" style={marketplaceStyles.linkButton}>
          Manage installed apps
        </Link>
      }
    >
      <MarketplaceNav
        canWrite={can('admin.marketplace.write')}
        canReview={can('admin.marketplace.review')}
      />
      {error ? <ErrorState message={error} /> : null}
      {!error && loading ? <LoadingState label="Loading marketplace catalogue" /> : null}
      {!error && !loading ? (
        <Table
          columns={[
            {
              key: 'name',
              header: 'App',
              cell: (listing: MarketplaceListing) => (
                <Link
                  href={`/marketplace/${encodeURIComponent(listing.id)}`}
                  style={{ color: 'var(--cos-color-accent)', fontWeight: 600 }}
                >
                  {listing.name}
                </Link>
              ),
            },
            {
              key: 'description',
              header: 'Description',
              minWidth: 260,
              cell: (listing: MarketplaceListing) => listing.description || '—',
            },
            {
              key: 'kind',
              header: 'Kind',
              cell: (listing: MarketplaceListing) => (
                <Badge tone="neutral">{listing.listing_kind ?? listing.kind ?? 'app'}</Badge>
              ),
            },
            {
              key: 'action',
              header: '',
              align: 'right',
              cell: (listing: MarketplaceListing) => (
                <Link
                  href={`/marketplace/${encodeURIComponent(listing.id)}`}
                  style={marketplaceStyles.linkButton}
                  aria-label={`Review and install ${listing.name}`}
                >
                  Install
                </Link>
              ),
            },
          ]}
          rows={listings}
          getRowKey={(listing) => listing.id}
          empty={
            <EmptyState
              title="No published apps"
              description="Approved marketplace listings will appear here."
            />
          }
        />
      ) : null}
    </MarketplacePage>
  );
}
