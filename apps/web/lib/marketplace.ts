import type { BadgeTone } from '@companyos/design-system';

export type MarketplaceListing = {
  id: string;
  name: string;
  slug: string;
  description: string;
  kind?: string;
  listing_kind?: string;
  requested_scopes: string[];
  redirect_uri?: string | null;
  redirect_uris?: string[];
  status?: string;
};

export type MarketplaceInstall = {
  id: string;
  listing_id?: string;
  app_id?: string;
  name?: string;
  app_name?: string;
  listing_name?: string;
  listing?: Pick<MarketplaceListing, 'id' | 'name' | 'slug' | 'kind'>;
  consented_scopes?: string[];
  scopes?: string[];
  status: string;
  installed_at?: string;
};

export type SecurityCheck = {
  id: string;
  label: string;
  required?: boolean;
  completed: boolean;
};

export type MarketplaceReview = {
  id: string;
  listing_id: string;
  listing_status: string;
  checklist: SecurityCheck[];
  security_review_completed: boolean;
  status: string;
  reviewer_notes?: string;
  listing?: MarketplaceListing;
  listing_name?: string;
  requested_scopes?: string[];
  redirect_uris?: string[];
};

export type Integration = {
  connector_key: string;
  name?: string;
  description?: string;
  status: string;
  scopes?: string[];
  requested_scopes?: string[];
  available_scopes?: string[];
  last_error?: string | null;
};

export function itemsFrom<T>(body: unknown, keys: string[]): T[] {
  if (Array.isArray(body)) return body as T[];
  if (!body || typeof body !== 'object') return [];
  const record = body as Record<string, unknown>;
  for (const key of ['items', ...keys]) {
    if (Array.isArray(record[key])) return record[key] as T[];
  }
  return [];
}

export async function responseMessage(response: Response, fallback: string): Promise<string> {
  try {
    const body = (await response.json()) as { detail?: string; message?: string; error?: string };
    return body.detail ?? body.message ?? body.error ?? fallback;
  } catch {
    return fallback;
  }
}

export function statusTone(status?: string): BadgeTone {
  switch (status?.toLowerCase()) {
    case 'active':
    case 'connected':
    case 'installed':
    case 'published':
    case 'approved':
      return 'success';
    case 'submitted':
    case 'pending':
    case 'in_review':
      return 'warning';
    case 'failed':
    case 'error':
    case 'rejected':
      return 'danger';
    default:
      return 'neutral';
  }
}

export function humanize(value: string): string {
  return value.replace(/[._-]+/g, ' ').replace(/\b\w/g, (letter) => letter.toUpperCase());
}
