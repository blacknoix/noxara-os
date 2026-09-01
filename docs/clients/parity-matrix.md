# Client parity matrix (Phase 4.5 / 1.11 surface)

Published matrix of **web** vs intended **mobile** vs **desktop** clients for the
1.11 product surface. Native Flutter / Tauri shells were **not** shipped in 1.11
and are not feature-complete in this phase — see status column.

CI references this file via `apps/web/lib/offline/parity-matrix.test.ts`.

## Conflict / offline rule (web)

- Mutations use stable `Idempotency-Key` so offline replay cannot duplicate
  expenses / invoices / other POSTs.
- Concurrent edits use optimistic `version` + `If-Match` (TRD §11).
- **Deterministic resolve:** last-write-wins with matching `If-Match` version.
  Stale writers receive **409 Conflict**; the loser is **not** silently dropped.
  The web shell shows a conflict alert and requires explicit acknowledgement.

## Matrix

| Feature | Web (`apps/web`) | Mobile (Flutter) | Desktop (Tauri) | Notes |
| --- | --- | --- | --- | --- |
| Auth (login / MFA / refresh) | implemented | not-yet | thin/not-yet | Web only |
| Org switch | implemented | not-yet | thin/not-yet | Web only |
| Dashboard | implemented | not-yet | thin/not-yet | Offline read-cache on web |
| Approvals (list / decide) | implemented | not-yet | thin/not-yet | Offline queue on web |
| Tasks (board / move) | implemented | not-yet | thin/not-yet | If-Match + offline queue |
| Deal quick-update | implemented | not-yet | thin/not-yet | If-Match on pipeline move |
| Expense capture | implemented | not-yet | thin/not-yet | Idempotency-Key + offline queue |
| Copilot | implemented | not-yet | thin/not-yet | Web AI panel |
| Industry pack install | implemented | not-yet | thin/not-yet | Settings → Industry packs |
| Offline conflict UI | implemented | not-yet | thin/not-yet | Banner + ConfirmDialog |
| Store-signed iOS/Android | out-of-scope | out-of-scope | n/a | Remains for a later native push |
| Crashlytics / APNs / FCM | out-of-scope | out-of-scope | out-of-scope | Not wired |

## Status legend

| Status | Meaning |
| --- | --- |
| `implemented` | Available in this repo and covered by tests where marked |
| `thin/not-yet` | Optional thin shell not shipped; parity row reserved |
| `not-yet` | Intended client does not exist yet |
| `out-of-scope` | Explicitly deferred (store submission, push providers) |

## What remains for native store work

1. Flutter app scaffold + feature parity against this matrix
2. Tauri desktop wrapper (Linux CI first; signed macOS/Windows later)
3. Store signing, Crashlytics, APNs/FCM — only after shells exist
