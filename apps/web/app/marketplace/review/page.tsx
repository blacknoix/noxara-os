'use client';

import { useCallback, useEffect, useState } from 'react';
import {
  Button,
  Card,
  Checkbox,
  EmptyState,
  ErrorState,
  LoadingState,
  PermissionDeniedState,
} from '@companyos/design-system';
import {
  MarketplaceNav,
  MarketplacePage,
  ScopeBadges,
  StatusBadge,
  marketplaceStyles,
} from '../../../components/MarketplacePage';
import { authFetch, getAccessToken } from '../../../lib/auth-client';
import { useCapabilities } from '../../../lib/capabilities';
import {
  itemsFrom,
  responseMessage,
  type MarketplaceListing,
  type SecurityCheck,
} from '../../../lib/marketplace';

const defaultChecks: SecurityCheck[] = [
  { key: 'scopes', label: 'Requested scopes are necessary and least-privileged', complete: false },
  { key: 'redirect_uri', label: 'Redirect URI ownership and HTTPS are verified', complete: false },
  { key: 'security', label: 'Security review is complete', complete: false },
];

export default function MarketplaceReviewPage() {
  const { can, loading: capabilitiesLoading } = useCapabilities();
  const [listings, setListings] = useState<MarketplaceListing[]>([]);
  const [checksByListing, setChecksByListing] = useState<Record<string, SecurityCheck[]>>({});
  const [loading, setLoading] = useState(true);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const canReview = can('admin.marketplace.review');

  const loadQueue = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const response = await authFetch('/api/v1/marketplace/review');
      if (!response.ok) {
        setError(await responseMessage(response, 'Could not load the marketplace review queue.'));
        return;
      }
      const body = await response.json();
      const queue = itemsFrom<MarketplaceListing>(body, ['listings', 'queue']);
      setListings(queue);
      setChecksByListing(
        Object.fromEntries(
          queue.map((listing) => [
            listing.id,
            listing.security_checklist?.length
              ? listing.security_checklist
              : defaultChecks.map((check) => ({ ...check })),
          ]),
        ),
      );
    } catch {
      setError('The marketplace review queue request failed.');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (!capabilitiesLoading && canReview) void loadQueue();
    if (!capabilitiesLoading && !canReview) setLoading(false);
  }, [capabilitiesLoading, canReview, loadQueue]);

  async function setCheck(listingId: string, key: string, complete: boolean) {
    const previous = checksByListing[listingId] ?? defaultChecks;
    const next = previous.map((check) => (check.key === key ? { ...check, complete } : check));
    setChecksByListing((current) => ({ ...current, [listingId]: next }));
    setBusyId(listingId);
    setError(null);
    try {
      const response = await authFetch(
        `/api/v1/marketplace/listings/${encodeURIComponent(listingId)}/review`,
        {
          method: 'PATCH',
          body: JSON.stringify({ security_checklist: next }),
        },
      );
      if (!response.ok) {
        setChecksByListing((current) => ({ ...current, [listingId]: previous }));
        setError(await responseMessage(response, 'Could not update the security checklist.'));
      }
    } catch {
      setChecksByListing((current) => ({ ...current, [listingId]: previous }));
      setError('The security checklist update failed.');
    } finally {
      setBusyId(null);
    }
  }

  async function publish(listing: MarketplaceListing) {
    setBusyId(listing.id);
    setError(null);
    try {
      const response = await authFetch(
        `/api/v1/marketplace/listings/${encodeURIComponent(listing.id)}/publish`,
        { method: 'POST' },
      );
      if (!response.ok) {
        setError(await responseMessage(response, `Could not publish ${listing.name}.`));
        return;
      }
      await loadQueue();
    } catch {
      setError('The publish request failed.');
    } finally {
      setBusyId(null);
    }
  }

  if (!getAccessToken()) {
    return <ErrorState title="Sign in required" message="Sign in to review marketplace apps." />;
  }
  if (capabilitiesLoading) return <LoadingState label="Loading marketplace permissions" />;
  if (!canReview) return <PermissionDeniedState requiredPermission="admin.marketplace.review" />;

  return (
    <MarketplacePage
      eyebrow="Marketplace governance"
      title="Review queue"
      description="Verify app access and security requirements before making a listing available."
    >
      <MarketplaceNav canWrite={can('admin.marketplace.write')} canReview />
      {error ? <ErrorState message={error} /> : null}
      {loading ? <LoadingState label="Loading review queue" /> : null}
      {!loading && listings.length === 0 ? (
        <EmptyState
          title="Review queue is clear"
          description="Submitted marketplace listings will appear here."
        />
      ) : null}
      {!loading ? (
        <div style={marketplaceStyles.cardGrid}>
          {listings.map((listing) => {
            const checks = checksByListing[listing.id] ?? defaultChecks;
            const securityComplete = checks.length > 0 && checks.every((check) => check.complete);
            return (
              <Card key={listing.id} as="article" style={marketplaceStyles.section}>
                <div style={marketplaceStyles.cardHeader}>
                  <div>
                    <h2 style={{ margin: 0, fontSize: '1.1rem' }}>{listing.name}</h2>
                    <p style={marketplaceStyles.muted}>{listing.description}</p>
                  </div>
                  <StatusBadge status={listing.status ?? 'submitted'} />
                </div>
                <div>
                  <strong style={{ fontSize: '0.8125rem' }}>Requested scopes</strong>
                  <div style={{ marginTop: 'var(--cos-space-2)' }}>
                    <ScopeBadges scopes={listing.requested_scopes ?? []} />
                  </div>
                </div>
                <p style={marketplaceStyles.muted}>
                  Redirect URI: <code>{listing.redirect_uri ?? 'Not provided'}</code>
                </p>
                <fieldset style={marketplaceStyles.fieldset}>
                  <legend style={marketplaceStyles.legend}>Security checklist</legend>
                  {checks.map((check) => (
                    <Checkbox
                      key={check.key}
                      label={check.label}
                      checked={check.complete}
                      disabled={busyId !== null}
                      onChange={(event) =>
                        void setCheck(listing.id, check.key, event.target.checked)
                      }
                    />
                  ))}
                </fieldset>
                <Button
                  loading={busyId === listing.id}
                  disabled={!securityComplete || busyId !== null}
                  onClick={() => void publish(listing)}
                >
                  Publish
                </Button>
              </Card>
            );
          })}
        </div>
      ) : null}
    </MarketplacePage>
  );
}
