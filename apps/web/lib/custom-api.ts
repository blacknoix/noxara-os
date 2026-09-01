import { authFetch } from './auth-client';

export type FieldDef = {
  name: string;
  label: string;
  type: string;
  required?: boolean;
  formula?: string;
  options?: string[];
  ref_target?: string;
};

export type EntityDefinition = {
  id: string;
  slug: string;
  label: string;
  description: string;
  fields: FieldDef[];
  status: string;
  published_version: number;
};

export type CustomRecord = {
  id: string;
  entity_slug: string;
  values: Record<string, unknown>;
  created_at: string;
  updated_at: string;
};

export async function listEntities(): Promise<EntityDefinition[]> {
  const res = await authFetch('/api/v1/custom/entities');
  if (!res.ok) return [];
  const body = (await res.json()) as { items: EntityDefinition[] };
  return body.items ?? [];
}

export async function createEntity(input: {
  slug: string;
  label: string;
  description?: string;
  fields: FieldDef[];
}): Promise<EntityDefinition | null> {
  const res = await authFetch('/api/v1/custom/entities', {
    method: 'POST',
    body: JSON.stringify(input),
  });
  if (!res.ok) return null;
  return (await res.json()) as EntityDefinition;
}

export async function publishEntity(id: string): Promise<boolean> {
  const res = await authFetch(`/api/v1/custom/entities/${id}/publish`, { method: 'POST' });
  return res.ok;
}

export async function deleteEntity(id: string, confirmSlug: string): Promise<boolean> {
  const res = await authFetch(`/api/v1/custom/entities/${id}`, {
    method: 'DELETE',
    body: JSON.stringify({ confirm_slug: confirmSlug }),
  });
  return res.ok;
}

export async function listRecords(slug: string): Promise<CustomRecord[]> {
  const res = await authFetch(`/api/v1/custom/records/${slug}`);
  if (!res.ok) return [];
  const body = (await res.json()) as { items: CustomRecord[] };
  return body.items ?? [];
}

export async function createRecord(
  slug: string,
  values: Record<string, unknown>,
): Promise<CustomRecord | null> {
  const res = await authFetch(`/api/v1/custom/records/${slug}`, {
    method: 'POST',
    body: JSON.stringify({ values }),
  });
  if (!res.ok) return null;
  return (await res.json()) as CustomRecord;
}

export async function exportPackage(): Promise<unknown | null> {
  const res = await authFetch('/api/v1/custom/packages/export');
  if (!res.ok) return null;
  return res.json();
}

export async function importPackage(pkg: unknown): Promise<boolean> {
  const res = await authFetch('/api/v1/custom/packages/import', {
    method: 'POST',
    body: JSON.stringify({ package: pkg }),
  });
  return res.ok;
}
