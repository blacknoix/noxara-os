#!/usr/bin/env node
import { readFileSync, existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { GENERATOR_VERSION } from './generate.mjs';
import { spawnSync } from 'node:child_process';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const metaPath = join(root, 'generated.meta.json');
if (!existsSync(metaPath)) {
  console.error('Python SDK drift: generated.meta.json missing — run generate');
  process.exit(1);
}
const before = readFileSync(metaPath, 'utf8');
const r = spawnSync('node', [join(root, 'scripts', 'generate.mjs')], { encoding: 'utf8' });
if (r.status !== 0) {
  console.error(r.stderr || r.stdout);
  process.exit(1);
}
const after = readFileSync(metaPath, 'utf8');
if (before !== after) {
  console.error('Python SDK drift: regenerate and commit packages/sdk-python');
  process.exit(1);
}
const meta = JSON.parse(after);
if (meta.generator !== GENERATOR_VERSION) {
  console.error(`Python SDK drift: generator pin mismatch ${meta.generator} vs ${GENERATOR_VERSION}`);
  process.exit(1);
}
const init = readFileSync(join(root, 'src', 'companyos_public', '__init__.py'), 'utf8');
if (!init.includes(GENERATOR_VERSION)) {
  console.error('Python SDK drift: __init__.py missing generator version');
  process.exit(1);
}
console.log('Python SDK drift check OK', createHash('sha256').update(after).digest('hex').slice(0, 12));
