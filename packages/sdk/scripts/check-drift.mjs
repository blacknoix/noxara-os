#!/usr/bin/env node
/**
 * Fail CI if live OpenAPI from core (when available) drifts from committed openapi.json.
 * Offline mode: ensure generated.ts matches openapi.json schemas.
 */
import { readFileSync, existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const committed = readFileSync(join(root, 'openapi.json'), 'utf8');
const hash = createHash('sha256').update(committed).digest('hex');

const doc = JSON.parse(committed);
const expectedHelloKeys = ['id', 'org_id', 'message', 'created_by'];
const keys = Object.keys(doc.components.schemas.Hello.properties);
for (const k of expectedHelloKeys) {
  if (!keys.includes(k)) {
    console.error(`OpenAPI drift: Hello schema missing ${k}`);
    process.exit(1);
  }
}

for (const name of ['TokenResponse', 'LoginRequest', 'RegisterRequest', 'SwitchOrgRequest']) {
  if (!doc.components.schemas[name]) {
    console.error(`OpenAPI drift: missing auth schema ${name}`);
    process.exit(1);
  }
}

if (!doc.paths['/api/v1/auth/login'] || !doc.paths['/api/v1/auth/register']) {
  console.error('OpenAPI drift: missing auth paths');
  process.exit(1);
}

if (!doc.paths['/api/v1/dashboard']) {
  console.error('OpenAPI drift: missing /api/v1/dashboard');
  process.exit(1);
}
for (const name of ['DashboardResponse', 'DashboardWidget']) {
  if (!doc.components.schemas[name]) {
    console.error(`OpenAPI drift: missing schema ${name}`);
    process.exit(1);
  }
}

if (!doc.paths['/api/v1/sales/pipelines']) {
  console.error('OpenAPI drift: missing /api/v1/sales/pipelines');
  process.exit(1);
}
if (!doc.components.schemas.DealDto && !doc.components.schemas.CustomerDto) {
  console.error('OpenAPI drift: missing DealDto or CustomerDto schema');
  process.exit(1);
}

if (!doc.paths['/api/v1/finance/invoices']) {
  console.error('OpenAPI drift: missing /api/v1/finance/invoices');
  process.exit(1);
}
if (!doc.components.schemas.InvoiceDto) {
  console.error('OpenAPI drift: missing InvoiceDto schema');
  process.exit(1);
}

if (!doc.paths['/api/v1/operations/projects']) {
  console.error('OpenAPI drift: missing /api/v1/operations/projects');
  process.exit(1);
}
for (const name of ['ProjectDto', 'TaskDto']) {
  if (!doc.components.schemas[name]) {
    console.error(`OpenAPI drift: missing schema ${name}`);
    process.exit(1);
  }
}

if (!existsSync(join(root, 'src/generated.ts'))) {
  console.error('Missing src/generated.ts — run pnpm generate:sdk');
  process.exit(1);
}

const gen = readFileSync(join(root, 'src/generated.ts'), 'utf8');
for (const k of expectedHelloKeys) {
  if (!gen.includes(k)) {
    console.error(`generated.ts drift: missing field ${k}`);
    process.exit(1);
  }
}
for (const name of ['TokenResponse', 'access_token', 'SwitchOrgRequest']) {
  if (!gen.includes(name)) {
    console.error(`generated.ts drift: missing ${name}`);
    process.exit(1);
  }
}

console.log(`openapi drift check ok (sha256 ${hash.slice(0, 12)}…)`);

const liveUrl = process.env.CORE_OPENAPI_URL;
if (liveUrl) {
  const res = await fetch(liveUrl);
  if (!res.ok) {
    console.error(`failed to fetch live openapi: ${res.status}`);
    process.exit(1);
  }
  const liveSchemas = JSON.parse(await res.text()).components.schemas;
  const committedSchemas = doc.components.schemas;
  if (JSON.stringify(liveSchemas) !== JSON.stringify(committedSchemas)) {
    console.error('OpenAPI schema drift between live core and packages/sdk/openapi.json');
    process.exit(1);
  }
  console.log('live OpenAPI schemas match committed file');
}
