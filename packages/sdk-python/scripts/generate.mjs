#!/usr/bin/env node
/**
 * Deterministic Python SDK generator from openapi.public.json.
 * Generator version is pinned below — bump intentionally when changing output.
 */
import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';

export const GENERATOR_VERSION = 'companyos-python-sdk-gen@1.0.0';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const workspaceRoot = join(root, '..', '..');
const publicSpecPath = join(workspaceRoot, 'packages', 'sdk', 'openapi.public.json');
const fallbackSpecPath = join(workspaceRoot, 'packages', 'sdk', 'openapi.json');

const specPath = existsSync(publicSpecPath) ? publicSpecPath : fallbackSpecPath;
const openapi = JSON.parse(readFileSync(specPath, 'utf8'));
const paths = openapi.paths ?? {};
const schemas = openapi.components?.schemas ?? {};

function pyType(v) {
  if (!v) return 'Any';
  if (v.$ref) return v.$ref.split('/').pop();
  if (v.type === 'array') return `list[${pyType(v.items)}]`;
  if (v.type === 'integer') return 'int';
  if (v.type === 'number') return 'float';
  if (v.type === 'boolean') return 'bool';
  if (v.type === 'object') return 'dict[str, Any]';
  return 'str';
}

const outDir = join(root, 'src', 'companyos_public');
mkdirSync(outDir, { recursive: true });

const schemaNames = Object.keys(schemas).sort();
const modelsLines = [
  `"""Generated models — ${GENERATOR_VERSION}. Do not edit."""`,
  'from __future__ import annotations',
  'from typing import Any, Optional',
  'from pydantic import BaseModel, ConfigDict',
  '',
];

for (const name of schemaNames) {
  const schema = schemas[name];
  if (!schema?.properties) {
    modelsLines.push(`class ${name}(BaseModel):`);
    modelsLines.push('    model_config = ConfigDict(extra="allow")');
    modelsLines.push('');
    continue;
  }
  const req = new Set(schema.required ?? []);
  modelsLines.push(`class ${name}(BaseModel):`);
  modelsLines.push('    model_config = ConfigDict(extra="allow")');
  for (const [k, v] of Object.entries(schema.properties)) {
    const opt = req.has(k) ? '' : ' | None = None';
    const deprecated = v.deprecated ? '  # deprecated' : '';
    modelsLines.push(`    ${k}: ${pyType(v)}${opt}${deprecated}`);
  }
  modelsLines.push('');
}

writeFileSync(join(outDir, 'models.py'), modelsLines.join('\n') + '\n');

const clientLines = [
  `"""Generated CompanyOS public API client — ${GENERATOR_VERSION}."""`,
  'from __future__ import annotations',
  'import urllib.request',
  'import json',
  'from typing import Any, Optional',
  '',
  'class CompanyOsPublicClient:',
  '    def __init__(self, base_url: str, api_key: str, timeout: float = 30.0):',
  '        self.base_url = base_url.rstrip("/")',
  '        self.api_key = api_key',
  '        self.timeout = timeout',
  '',
  '    def _request(self, method: str, path: str, body: Optional[dict[str, Any]] = None, idempotency_key: Optional[str] = None) -> Any:',
  '        data = None if body is None else json.dumps(body).encode("utf-8")',
  '        headers = {',
  '            "Authorization": f"Bearer {self.api_key}",',
  '            "Accept": "application/json",',
  '            "Content-Type": "application/json",',
  '        }',
  '        if idempotency_key:',
  '            headers["Idempotency-Key"] = idempotency_key',
  '        req = urllib.request.Request(self.base_url + path, data=data, headers=headers, method=method)',
  '        with urllib.request.urlopen(req, timeout=self.timeout) as resp:',
  '            raw = resp.read().decode("utf-8")',
  '            return json.loads(raw) if raw else None',
  '',
  '    def list_customers(self) -> Any:',
  '        return self._request("GET", "/api/v1/sales/customers")',
  '',
  '    def create_customer(self, body: dict[str, Any], idempotency_key: str) -> Any:',
  '        return self._request("POST", "/api/v1/sales/customers", body, idempotency_key)',
  '',
  '    def list_invoices(self) -> Any:',
  '        return self._request("GET", "/api/v1/finance/invoices")',
  '',
  '    def create_invoice(self, body: dict[str, Any], idempotency_key: str) -> Any:',
  '        return self._request("POST", "/api/v1/finance/invoices", body, idempotency_key)',
  '',
];

writeFileSync(join(outDir, 'client.py'), clientLines.join('\n') + '\n');
writeFileSync(
  join(outDir, '__init__.py'),
  [
    `"""CompanyOS official public Python SDK (${GENERATOR_VERSION})."""`,
    'from .client import CompanyOsPublicClient',
    '__all__ = ["CompanyOsPublicClient"]',
    `__generator_version__ = "${GENERATOR_VERSION}"`,
    '',
  ].join('\n'),
);

const meta = {
  generator: GENERATOR_VERSION,
  source: specPath.includes('openapi.public') ? 'openapi.public.json' : 'openapi.json',
  path_count: Object.keys(paths).length,
  schema_count: schemaNames.length,
  content_sha256: createHash('sha256')
    .update(JSON.stringify({ paths: Object.keys(paths).sort(), schemas: schemaNames }))
    .digest('hex'),
};
writeFileSync(join(root, 'generated.meta.json'), JSON.stringify(meta, null, 2) + '\n');
console.log(`Python SDK generated (${GENERATOR_VERSION}) from ${meta.source}`);
