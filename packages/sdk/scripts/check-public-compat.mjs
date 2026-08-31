#!/usr/bin/env node
/**
 * Backwards-compatibility test against the frozen previous public OpenAPI.
 * Asserts deprecation dual-publish of rate_limit_rpm and that public paths
 * remain additive (no removals vs baseline).
 */
import { readFileSync, existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const currentPath = join(root, 'openapi.public.json');
const previousPath = join(root, 'openapi.public.previous.json');

if (!existsSync(currentPath)) {
  console.error('Missing openapi.public.json');
  process.exit(1);
}
if (!existsSync(previousPath)) {
  console.error('Missing openapi.public.previous.json — freeze baseline via export-openapi.sh');
  process.exit(1);
}

const current = JSON.parse(readFileSync(currentPath, 'utf8'));
const previous = JSON.parse(readFileSync(previousPath, 'utf8'));

const prevPaths = new Set(Object.keys(previous.paths || {}));
const curPaths = new Set(Object.keys(current.paths || {}));
for (const p of prevPaths) {
  if (!curPaths.has(p)) {
    console.error(`Public compatibility break: removed path ${p}`);
    process.exit(1);
  }
}

// Deprecation exercise: dual-published rate_limit_rpm must remain until Sunset.
const ex =
  current.components?.schemas?.ApiKeyExchangeResponse ||
  previous.components?.schemas?.ApiKeyExchangeResponse;
if (ex?.properties) {
  if (!ex.properties.rate_limit_per_minute) {
    console.error('Missing rate_limit_per_minute on ApiKeyExchangeResponse');
    process.exit(1);
  }
  if (!ex.properties.rate_limit_rpm) {
    // Allow previous baseline without the field only if current introduces it as deprecated.
    const curEx = current.components?.schemas?.ApiKeyExchangeResponse;
    if (!curEx?.properties?.rate_limit_rpm?.deprecated) {
      console.error('Deprecation exercise failed: rate_limit_rpm must be dual-published as deprecated');
      process.exit(1);
    }
  } else if (ex.properties.rate_limit_rpm && current.components?.schemas?.ApiKeyExchangeResponse) {
    const rpm = current.components.schemas.ApiKeyExchangeResponse.properties.rate_limit_rpm;
    if (!rpm?.deprecated) {
      console.error('rate_limit_rpm must be marked deprecated: true');
      process.exit(1);
    }
  }
}

if (!current['x-companyos-deprecation-policy']?.window_days) {
  console.error('Missing x-companyos-deprecation-policy.window_days');
  process.exit(1);
}

console.log('Public OpenAPI compatibility check OK');
