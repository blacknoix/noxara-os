'use client';

import { useCallback, useEffect, useState } from 'react';
import Link from 'next/link';
import { useParams } from 'next/navigation';
import {
  Badge,
  Button,
  Checkbox,
  ErrorState,
  InlineAlert,
  LoadingState,
  PermissionDeniedState,
} from '@companyos/design-system';
import {
  MarketplaceNav,
  MarketplacePage,
  ScopeBadges,
  marketplaceStyles,
} from '../../../components/MarketplacePage';
import { authFetch, getAccessToken } from '../../../lib/auth-client';
import { useCapabilities } from '../../../lib/capabilities';
import {
  responseMessage,
  type MarketplaceInstall,
  type MarketplaceListing,
} from '../../../lib/marketplace';

export default function MarketplaceListingPage() {
  const params = useParams<{ id: string }>();
  const listingId = Array.isArray(params.id) ? params.id[0] : params.id;
  const { can, loading: capabilitiesLoading } = useCapabilities();
  const [listing, setListing] = useState<MarketplaceListing | null>(null);
  const [selectedScopes, setSelectedScopes] = useState<string[]>([]);
  const [installedScopes, setInstalledScopes] = useState<string[] | null>(null);
  const [loading, setLoading] = useState(true);
  const [installing, setInstalling] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const canRead = can('admin.marketplace.read');
  const canInstall = can('admin.marketplace.install');

  const loadListing = useCallback(async () => {
    if (!listingId) return;
    setLoading(true);
    setError(null);
    try {
      const response = await authFetch(
        `/api/v1/marketplace/catalogue/${encodeURIComponent(listingId)}`,
      );
      if (!response.ok) {
        setError(await responseMessage(response, 'Could not load this marketplace listing.'));
        return;
      }
      const body = (await response.json()) as
        | MarketplaceListing
        | { listing: MarketplaceListing };
      const nextListing = 'listing' in body ? body.listing : body;
      setListing(nextListing);
      setSelectedScopes(nextListing.requested_scopes ?? []);
    } catch {
      setError('The marketplace listing request failed.');
    } finally {
      setLoading(false);
    }
  }, [listingId]);

  useEffect(() => {
    if (!capabilitiesLoading && canRead) void loadListing();
    if (!capabilitiesLoading && !canRead) setLoading(false);
  }, [capabilitiesLoading, canRead, loadListing]);

  function toggleScope(scope: string) {
    setSelectedScopes((current) =>
      current.includes(scope) ? current.filter((item) => item !== scope) : [...current, scope],
    );
  }

  async function install() {
    if (!listing || !canInstall) return;
    setInstalling(true);
    setError(null);
    try {
      const response = await authFetch('/api/v1/marketplace/installs', {
        method: 'POST',
        body: JSON.stringify({
          listing_id: listing.id,
          consented_scopes: selectedScopes,
        }),
      });
      if (!response.ok) {
        setError(await responseMessage(response, 'Could not install this app.'));
        return;
      }
      const body = (await response.json()) as MarketplaceInstall;
      setInstalledScopes(body.consented_scopes ?? body.scopes ?? selectedScopes);
    } catch {
      setError('The install request failed.');
    } finally {
      setInstalling(false);
    }
  }

  if (!getAccessToken()) {
    return <ErrorState title="Sign in required" message="Sign in to review this app." />;
  }
  if (capabilitiesLoading) return <LoadingState label="Loading marketplace permissions" />;
  if (!canRead) return <PermissionDeniedState requiredPermission="admin.marketplace.read" />;

  return (
    <MarketplacePage
      title={listing?.name ?? 'App details'}
      description={listing?.description ?? 'Review this listing and the access it requests.'}
      actions={
        <Link href="/marketplace" style={marketplaceStyles.linkButton}>
          Back to catalogue
        </Link>
      }
    >
      <MarketplaceNav
        canWrite={can('admin.marketplace.write')}
        canReview={can('admin.marketplace.review')}
      />
      {error ? <ErrorState message={error} /> : null}
      {!error && loading ? <LoadingState label="Loading app details" /> : null}
      {!error && !loading && listing ? (
        <section style={marketplaceStyles.section} aria-labelledby="install-consent-title">
          <div style={marketplaceStyles.row}>
            <Badge tone="neutral">{listing.kind ?? 'app'}</Badge>
            {listing.slug ? <span style={marketplaceStyles.muted}>{listing.slug}</span> : null}
          </div>

          {installedScopes ? (
            <InlineAlert tone="success" title={`${listing.name} installed`}>
              <div style={{ display: 'grid', gap: 'var(--cos-space-2)' }}>
                <span>The app was installed with these consented scopes:</span>
                <ScopeBadges scopes={installedScopes} />
              </div>
            </InlineAlert>
          ) : (
            <>
              <fieldset style={marketplaceStyles.fieldset}>
                <legend id="install-consent-title" style={marketplaceStyles.legend}>
                  Requested access
                </legend>
                <p style={marketplaceStyles.muted}>
                  Clear any optional scope you do not want to grant. All scopes are selected by
                  default.
                </p>
                {listing.requested_scopes.length === 0 ? (
                  <span style={marketplaceStyles.muted}>This app does not request any scopes.</span>
                ) : (
                  listing.requested_scopes.map((scope) => (
                    <Checkbox
                      key={scope}
                      label={scope}
                      checked={selectedScopes.includes(scope)}
                      onChange={() => toggleScope(scope)}
                      disabled={!canInstall || installing}
                    />
                  ))
                )}
              </fieldset>
              {canInstall ? (
                <Button onClick={() => void install()} loading={installing}>
                  Install {listing.name}
                </Button>
              ) : (
                <PermissionDeniedState
                  title="Installation unavailable"
                  message="You can review this listing, but your role cannot install apps."
                  requiredPermission="admin.marketplace.install"
                />
              )}
            </>
          )}
        </section>
      ) : null}
    </MarketplacePage>
  );
}
