'use client';

import Link from 'next/link';

export default function DevelopersPage() {
  return (
    <main style={{ maxWidth: 720, margin: '2rem auto', padding: '0 1.25rem', lineHeight: 1.55 }}>
      <h1 style={{ fontSize: '1.75rem', marginBottom: '0.5rem' }}>Developers</h1>
      <p style={{ opacity: 0.85 }}>
        Build integrations against the CompanyOS public API with organization API keys,
        versioned OpenAPI, and outbound webhooks.
      </p>
      <ul>
        <li>
          <Link href="/docs/developers">In-repo developer docs</Link> (auth, scopes, webhooks,
          quickstart, sandbox)
        </li>
        <li>
          Public OpenAPI: <code>/api/v1/openapi.public.json</code>
        </li>
        <li>
          SDKs: <code>@companyos/sdk</code> (TypeScript) and <code>companyos_public</code>{' '}
          (Python)
        </li>
        <li>
          Manage keys under <Link href="/settings/security">Settings → Security</Link>
        </li>
      </ul>
      <p style={{ fontSize: '0.9rem', opacity: 0.75 }}>
        Hosted developer portal billing is out of scope for Phase 3.3. Use the sandbox seed script
        for local keys.
      </p>
    </main>
  );
}
