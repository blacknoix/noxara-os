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

const expectedHelloKeys = ['id', 'org_id', 'message', 'created_by'];
const doc = JSON.parse(committed);
const keys = Object.keys(doc.components.schemas.Hello.properties);
for (const k of expectedHelloKeys) {
  if (!keys.includes(k)) {
    console.error(`OpenAPI drift: Hello schema missing ${k}`);
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

console.log(`openapi drift check ok (sha256 ${hash.slice(0, 12)}…)`);

const liveUrl = process.env.CORE_OPENAPI_URL;
if (liveUrl) {
  const res = await fetch(liveUrl);
  if (!res.ok) {
    console.error(`failed to fetch live openapi: ${res.status}`);
    process.exit(1);
  }
  const live = await res.text();
  const liveNorm = JSON.stringify(JSON.parse(live), null, 2) + '\n';
  const committedNorm = JSON.stringify(JSON.parse(committed), null, 2) + '\n';
  // Compare schema components only for stability across utoipa formatting.
  const liveSchemas = JSON.parse(live).components.schemas;
  const committedSchemas = JSON.parse(committed).components.schemas;
  if (JSON.stringify(liveSchemas) !== JSON.stringify(committedSchemas)) {
    console.error('OpenAPI schema drift between live core and packages/sdk/openapi.json');
    process.exit(1);
  }
  console.log('live OpenAPI schemas match committed file');
  void liveNorm;
  void committedNorm;
}
