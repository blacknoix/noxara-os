/* CompanyOS offline-first service worker (Phase 4.5).
 * Caches selected GETs for dashboard / tasks / approvals.
 * Mutations are handled by the app offline queue (Idempotency-Key), not here.
 */
const CACHE = 'companyos-read-v1';
const READ_PREFIXES = [
  '/api/v1/workspace/dashboard',
  '/api/v1/operations/tasks',
  '/api/v1/operations/approvals',
];

self.addEventListener('install', (event) => {
  event.waitUntil(self.skipWaiting());
});

self.addEventListener('activate', (event) => {
  event.waitUntil(self.clients.claim());
});

self.addEventListener('fetch', (event) => {
  const req = event.request;
  if (req.method !== 'GET') return;
  const url = new URL(req.url);
  const match = READ_PREFIXES.some((p) => url.pathname.startsWith(p));
  if (!match) return;

  event.respondWith(
    (async () => {
      try {
        const res = await fetch(req);
        if (res.ok) {
          const cache = await caches.open(CACHE);
          cache.put(req, res.clone());
        }
        return res;
      } catch {
        const cached = await caches.match(req);
        if (cached) return cached;
        return new Response(JSON.stringify({ offline: true, items: [] }), {
          status: 503,
          headers: { 'Content-Type': 'application/json' },
        });
      }
    })(),
  );
});
