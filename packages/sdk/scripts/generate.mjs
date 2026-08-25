#!/usr/bin/env node
/**
 * Generate TypeScript types from openapi.json (Phase 0: lightweight emitter).
 * Full openapi-typescript can replace this later; CI drift check uses the committed JSON.
 */
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const openapi = JSON.parse(readFileSync(join(root, 'openapi.json'), 'utf8'));
const hello = openapi.components.schemas.Hello;
const create = openapi.components.schemas.CreateHelloRequest;
const list = openapi.components.schemas.HelloListResponse;

function propsToTs(schema) {
  const req = new Set(schema.required ?? []);
  return Object.entries(schema.properties)
    .map(([k, v]) => {
      const opt = req.has(k) ? '' : '?';
      const desc = v.description ? `  /** ${v.description} */\n` : '';
      if (v.type === 'array') {
        return `${desc}  ${k}${opt}: Hello[];`;
      }
      return `${desc}  ${k}${opt}: string;`;
    })
    .join('\n');
}

const banner = `/** AUTO-GENERATED from openapi.json — do not edit by hand. Run pnpm generate:sdk */\n`;
const types = `${banner}
export type Hello = {
${propsToTs(hello)}
};

export type CreateHelloRequest = {
${propsToTs(create)}
};

export type HelloListResponse = {
${propsToTs(list)}
};
`;

writeFileSync(join(root, 'src/generated.ts'), types);
console.log('wrote src/generated.ts');
