'use client';

import { useCallback, useEffect, useState, type FormEvent } from 'react';
import {
  Button,
  EmptyState,
  ErrorState,
  Input,
  LoadingState,
  PermissionDeniedState,
  Table,
  Textarea,
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
import { itemsFrom, responseMessage, type MarketplaceListing } from '../../../lib/marketplace';

const initialForm = {
  name: '',
  slug: '',
  description: '',
  requestedScopes: '',
  redirectUri: '',
};

export default function MarketplacePublisherPage() {
  const { can, loading: capabilitiesLoading } = useCapabilities();
  const [listings, setListings] = useState<MarketplaceListing[]>([]);
  const [form, setForm] = useState(initialForm);
  const [loading, setLoading] = useState(true);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const canWrite = can('admin.marketplace.write');

  const loadMine = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const response = await authFetch('/api/v1/marketplace/listings/mine');
      if (!response.ok) {
        setError(await responseMessage(response, 'Could not load your marketplace listings.'));
        return;
      }
      const body = await response.json();
      setListings(itemsFrom<MarketplaceListing>(body, ['listings']));
    } catch {
      setError('The publisher listings request failed.');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (!capabilitiesLoading && canWrite) void loadMine();
    if (!capabilitiesLoading && !canWrite) setLoading(false);
  }, [capabilitiesLoading, canWrite, loadMine]);

  async function submitListing(listingId: string): Promise<boolean> {
    setBusyId(listingId);
    const response = await authFetch(
      `/api/v1/marketplace/listings/${encodeURIComponent(listingId)}/submit`,
      { method: 'POST' },
    );
    if (!response.ok) {
      setError(await responseMessage(response, 'Could not submit the listing for review.'));
      setBusyId(null);
      return false;
    }
    setBusyId(null);
    return true;
  }

  async function createAndSubmit(event: FormEvent) {
    event.preventDefault();
    setBusyId('new');
    setError(null);
    const requestedScopes = form.requestedScopes
      .split(',')
      .map((scope) => scope.trim())
      .filter(Boolean);
    try {
      const response = await authFetch('/api/v1/marketplace/listings', {
        method: 'POST',
        body: JSON.stringify({
          name: form.name.trim(),
          slug: form.slug.trim(),
          description: form.description.trim(),
          requested_scopes: requestedScopes,
          redirect_uri: form.redirectUri.trim(),
        }),
      });
      if (!response.ok) {
        setError(await responseMessage(response, 'Could not create the marketplace listing.'));
        return;
      }
      const body = (await response.json()) as MarketplaceListing | { listing: MarketplaceListing };
      const created = 'listing' in body ? body.listing : body;
      if (!created.id) {
        setError('The listing was created, but its identifier was missing from the response.');
        return;
      }
      if (!(await submitListing(created.id))) return;
      setForm(initialForm);
      await loadMine();
    } catch {
      setError('The listing submission request failed.');
    } finally {
      setBusyId(null);
    }
  }

  async function resubmit(listingId: string) {
    setError(null);
    try {
      if (await submitListing(listingId)) await loadMine();
    } catch {
      setError('The listing submission request failed.');
      setBusyId(null);
    }
  }

  if (!getAccessToken()) {
    return <ErrorState title="Sign in required" message="Sign in to publish marketplace apps." />;
  }
  if (capabilitiesLoading) return <LoadingState label="Loading marketplace permissions" />;
  if (!canWrite) return <PermissionDeniedState requiredPermission="admin.marketplace.write" />;

  return (
    <MarketplacePage
      eyebrow="Marketplace publisher"
      title="Publish an app"
      description="Create a listing and submit its requested access for security review."
    >
      <MarketplaceNav canWrite canReview={can('admin.marketplace.review')} />
      {error ? <ErrorState message={error} /> : null}

      <form onSubmit={(event) => void createAndSubmit(event)} style={marketplaceStyles.form}>
        <Input
          label="App name"
          value={form.name}
          required
          onChange={(event) => setForm({ ...form, name: event.target.value })}
        />
        <Input
          label="Slug"
          value={form.slug}
          required
          pattern="[a-z0-9]+(?:-[a-z0-9]+)*"
          hint="Lowercase letters, numbers, and hyphens."
          onChange={(event) => setForm({ ...form, slug: event.target.value })}
        />
        <Textarea
          label="Description"
          value={form.description}
          required
          onChange={(event) => setForm({ ...form, description: event.target.value })}
        />
        <Input
          label="Requested scopes"
          value={form.requestedScopes}
          required
          hint="Comma-separated permission scopes."
          placeholder="sales.customer.read, operations.task.read"
          onChange={(event) => setForm({ ...form, requestedScopes: event.target.value })}
        />
        <Input
          label="Redirect URI"
          type="url"
          value={form.redirectUri}
          required
          placeholder="https://example.com/oauth/callback"
          onChange={(event) => setForm({ ...form, redirectUri: event.target.value })}
        />
        <Button type="submit" loading={busyId === 'new'} disabled={busyId !== null}>
          Submit for review
        </Button>
      </form>

      <section style={marketplaceStyles.section} aria-labelledby="my-listings-heading">
        <h2 id="my-listings-heading" style={{ margin: 0, fontSize: '1.15rem' }}>
          My listings
        </h2>
        {loading ? <LoadingState label="Loading your listings" /> : null}
        {!loading ? (
          <Table
            columns={[
              { key: 'name', header: 'Name', cell: (listing: MarketplaceListing) => listing.name },
              {
                key: 'scopes',
                header: 'Requested scopes',
                minWidth: 240,
                cell: (listing: MarketplaceListing) => (
                  <ScopeBadges scopes={listing.requested_scopes ?? []} />
                ),
              },
              {
                key: 'status',
                header: 'Status',
                cell: (listing: MarketplaceListing) => <StatusBadge status={listing.status} />,
              },
              {
                key: 'actions',
                header: '',
                align: 'right',
                cell: (listing: MarketplaceListing) =>
                  !listing.status ||
                  ['draft', 'rejected'].includes(listing.status.toLowerCase()) ? (
                    <Button
                      size="sm"
                      variant="secondary"
                      loading={busyId === listing.id}
                      disabled={busyId !== null}
                      onClick={() => void resubmit(listing.id)}
                    >
                      Submit for review
                    </Button>
                  ) : (
                    '—'
                  ),
              },
            ]}
            rows={listings}
            getRowKey={(listing) => listing.id}
            empty={
              <EmptyState
                title="No listings yet"
                description="Complete the form to submit your first app."
              />
            }
          />
        ) : null}
      </section>
    </MarketplacePage>
  );
}
