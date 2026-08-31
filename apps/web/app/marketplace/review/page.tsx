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
  type MarketplaceReview,
  type SecurityCheck,
} from '../../../lib/marketplace';

const defaultChecks: SecurityCheck[] = [
  {
    id: 'security_scopes',
    label: 'Requested scopes are necessary and least-privileged',
    required: true,
    completed: false,
  },
  {
    id: 'security_redirect_uri',
    label: 'Redirect URI ownership and HTTPS are verified',
    required: true,
    completed: false,
  },
  {
    id: 'security_review',
    label: 'Security review is complete',
    required: true,
    completed: false,
  },
];

export default function MarketplaceReviewPage() {
  const { can, loading: capabilitiesLoading } = useCapabilities();
  const [reviews, setReviews] = useState<MarketplaceReview[]>([]);
  const [checksByReview, setChecksByReview] = useState<Record<string, SecurityCheck[]>>({});
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
      const queue = itemsFrom<MarketplaceReview>(body, ['reviews', 'queue']);
      setReviews(queue);
      setChecksByReview(
        Object.fromEntries(
          queue.map((review) => [
            review.id,
            review.checklist?.length
              ? review.checklist
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

  async function setCheck(review: MarketplaceReview, checkId: string, completed: boolean) {
    const previous = checksByReview[review.id] ?? defaultChecks;
    const next = previous.map((check) => (check.id === checkId ? { ...check, completed } : check));
    setChecksByReview((current) => ({ ...current, [review.id]: next }));
    setBusyId(review.id);
    setError(null);
    try {
      const response = await authFetch(
        `/api/v1/marketplace/listings/${encodeURIComponent(review.listing_id)}/review`,
        {
          method: 'PATCH',
          body: JSON.stringify({
            completed_item_ids: next.filter((check) => check.completed).map((check) => check.id),
          }),
        },
      );
      if (!response.ok) {
        setChecksByReview((current) => ({ ...current, [review.id]: previous }));
        setError(await responseMessage(response, 'Could not update the security checklist.'));
      }
    } catch {
      setChecksByReview((current) => ({ ...current, [review.id]: previous }));
      setError('The security checklist update failed.');
    } finally {
      setBusyId(null);
    }
  }

  async function publish(review: MarketplaceReview) {
    const name = review.listing?.name ?? review.listing_name ?? review.listing_id;
    setBusyId(review.id);
    setError(null);
    try {
      const response = await authFetch(
        `/api/v1/marketplace/listings/${encodeURIComponent(review.listing_id)}/publish`,
        { method: 'POST' },
      );
      if (!response.ok) {
        setError(await responseMessage(response, `Could not publish ${name}.`));
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
      {!loading && reviews.length === 0 ? (
        <EmptyState
          title="Review queue is clear"
          description="Submitted marketplace listings will appear here."
        />
      ) : null}
      {!loading ? (
        <div style={marketplaceStyles.cardGrid}>
          {reviews.map((review) => {
            const listing = review.listing;
            const checks = checksByReview[review.id] ?? defaultChecks;
            const securityChecks = checks.filter((check) => check.id.startsWith('security_'));
            const securityComplete =
              review.security_review_completed ||
              (securityChecks.length > 0 && securityChecks.every((check) => check.completed));
            const name = listing?.name ?? review.listing_name ?? review.listing_id;
            const scopes = listing?.requested_scopes ?? review.requested_scopes ?? [];
            const redirectUris = listing?.redirect_uris ?? review.redirect_uris ?? [];
            return (
              <Card key={review.id} as="article" style={marketplaceStyles.section}>
                <div style={marketplaceStyles.cardHeader}>
                  <div>
                    <h2 style={{ margin: 0, fontSize: '1.1rem' }}>{name}</h2>
                    <p style={marketplaceStyles.muted}>
                      {listing?.description ?? `Listing ${review.listing_id}`}
                    </p>
                  </div>
                  <StatusBadge status={review.listing_status ?? review.status} />
                </div>
                <div>
                  <strong style={{ fontSize: '0.8125rem' }}>Requested scopes</strong>
                  <div style={{ marginTop: 'var(--cos-space-2)' }}>
                    <ScopeBadges scopes={scopes} />
                  </div>
                </div>
                <p style={marketplaceStyles.muted}>
                  Redirect URI: <code>{redirectUris.join(', ') || 'Not provided'}</code>
                </p>
                <fieldset style={marketplaceStyles.fieldset}>
                  <legend style={marketplaceStyles.legend}>Security checklist</legend>
                  {checks.map((check) => (
                    <Checkbox
                      key={check.id}
                      label={`${check.label}${check.required ? ' (required)' : ''}`}
                      checked={check.completed}
                      disabled={busyId !== null}
                      onChange={(event) => void setCheck(review, check.id, event.target.checked)}
                    />
                  ))}
                </fieldset>
                <Button
                  loading={busyId === review.id}
                  disabled={!securityComplete || busyId !== null}
                  onClick={() => void publish(review)}
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
