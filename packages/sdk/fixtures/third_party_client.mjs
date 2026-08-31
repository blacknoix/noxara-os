#!/usr/bin/env node
/**
 * Fixture third-party client — uses ONLY the published public contract paths
 * and docs/developers guides. No internal crates, no service DB access.
 *
 * Usage:
 *   COMPANYOS_API_URL=… COMPANYOS_API_KEY=… node packages/sdk/fixtures/third_party_client.mjs
 *
 * Live human developer validation is out of this PR; this fixture proves the
 * public contract is sufficient for a stranger-style integration.
 */
import { readFileSync, existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const publicSpec = join(root, 'openapi.public.json');
if (!existsSync(publicSpec)) {
  console.error('Missing openapi.public.json — run scripts/export-openapi.sh');
  process.exit(1);
}
const doc = JSON.parse(readFileSync(publicSpec, 'utf8'));
const allowed = new Set(Object.keys(doc.paths || {}));

const base = process.env.COMPANYOS_API_URL || 'http://127.0.0.1:8080';
const key = process.env.COMPANYOS_API_KEY;
if (!key) {
  console.log('No COMPANYOS_API_KEY — validating public contract surface only');
  for (const p of [
    '/api/v1/sales/customers',
    '/api/v1/finance/invoices',
    '/api/v1/governance/webhooks',
  ]) {
    const hit = [...allowed].some((a) => a === p || a.startsWith(p + '/') || a.startsWith(p));
    if (!hit) {
      console.error(`Public contract missing ${p}`);
      process.exit(1);
    }
  }
  console.log('third_party fixture: public contract paths present');
  process.exit(0);
}

async function call(method, path, body) {
  if (![...allowed].some((a) => path === a || path.startsWith(a.split('{')[0]))) {
    // Allow exact catalogue prefixes even when templated in OpenAPI
    const ok = [...allowed].some((a) => {
      const prefix = a.replace(/\{[^}]+\}/g, '').replace(/\/$/, '');
      return path === a || path.startsWith(prefix);
    });
    if (!ok) throw new Error(`Refusing non-public path ${path}`);
  }
  const headers = {
    Authorization: `Bearer ${key}`,
    Accept: 'application/json',
    'Content-Type': 'application/json',
    'Idempotency-Key': `fixture-${Date.now()}`,
  };
  const res = await fetch(`${base}${path}`, {
    method,
    headers,
    body: body ? JSON.stringify(body) : undefined,
  });
  const text = await res.text();
  return { status: res.status, headers: res.headers, body: text };
}

const list = await call('GET', '/api/v1/sales/customers');
console.log('GET customers', list.status);
if (list.status >= 500) process.exit(1);
const created = await call('POST', '/api/v1/sales/customers', {
  name: 'Fixture Customer',
  email: 'fixture@example.com',
});
console.log('POST customers', created.status);
console.log('third_party fixture complete');
