# Client parity matrix (Phase 4.5 / 1.11 surface)

Published matrix of **web** vs **mobile (Flutter)** vs **desktop (Tauri)** for the
1.11 high-frequency product surface. Native shells ship in Phase 1.11 as a
**subset** of web (PRD 5.8 / high-frequency set) — not full Phase 2–4 module
parity.

CI references this file via `apps/web/lib/offline/parity-matrix.test.ts` and
Flutter / desktop-shell unit tests.

## Conflict / offline rule (web + Flutter)

- Mutations use stable `Idempotency-Key` so offline replay cannot duplicate
  expenses / invoices / other POSTs.
- Concurrent edits use optimistic `version` + `If-Match` (TRD §11).
- **Deterministic resolve:** last-write-wins with matching `If-Match` version.
  Stale writers receive **409 Conflict**; the loser is **not** silently dropped.
  The web shell shows a conflict alert and requires explicit acknowledgement.
  Flutter queues with the same key semantics; conflict UI is thinner on mobile.

## Matrix

| Feature | Web (`apps/web`) | Mobile (Flutter `apps/mobile`) | Desktop (Tauri `apps/desktop`) | Notes |
| --- | --- | --- | --- | --- |
| Auth (login / MFA / refresh) | implemented | implemented | thin/wrap-web | Mobile uses `/api/v1/auth/*`; MFA challenge surfaced; desktop wraps web auth |
| Org switch | implemented | implemented | thin/wrap-web | `POST /auth/switch-org` → new JWT; deep links may switch org first |
| Dashboard | implemented | implemented | thin/wrap-web + offline cache | Flutter read-cache; Tauri offline shell shows last cached dashboard |
| Approvals (list / decide) | implemented | implemented | thin/wrap-web | Work tab; Idempotency-Key on decide |
| Tasks (board / move) | implemented | implemented | thin/wrap-web | List + navigate; not full board parity |
| Deal quick-update | implemented | implemented | thin/wrap-web | Patch stage with If-Match when version known |
| Expense capture | implemented | implemented | thin/wrap-web | Camera-first mobile; offline queue + same Idempotency-Key |
| Copilot | implemented | not-yet | thin/wrap-web | Desktop global hotkey ⌥Space / Alt+Space focuses web copilot |
| Industry pack install | implemented | not-yet | thin/wrap-web | Settings on web; not in 1.11 mobile tabs |
| Offline conflict UI | implemented | thin | thin/wrap-web | Flutter: queue + key stability; full conflict dialog later |
| Push notifications | n/a (SSE) | interface + fake | native notify API | Device token register API; **no live FCM/APNs in CI** |
| Biometric unlock | n/a | interface + fake | n/a | FakeBiometricService in CI; real biometrics are a store follow-up |
| Deep links `companyos://record/{id}` | n/a | implemented | implemented | Org via `?org=` / path; unit-tested open-in-correct-org |
| Store-signed iOS/Android/macOS | out-of-scope | out-of-scope | out-of-scope | Linux CI only; Xcode/signed artifacts deferred |
| Crashlytics / APNs / FCM live | out-of-scope | out-of-scope | out-of-scope | Not wired |

## Status legend

| Status | Meaning |
| --- | --- |
| `implemented` | Available in this repo and covered by tests where marked |
| `thin/wrap-web` | Tauri loads `apps/web`; shell features (tray, hotkey, deep link, offline cache) are native |
| `thin` | Partial client support vs web |
| `not-yet` | Intentionally not in this 1.11 slice |
| `out-of-scope` | Explicitly deferred (store submission, push providers, crash reporting) |

## What remains for native store work

1. Store-signed iOS / Android / macOS artifacts and notarization
2. Live Crashlytics / Sentry, APNs, FCM credentials
3. Broader mobile feature parity with Phase 2–4 modules (later client-parity)
4. Full Flutter conflict UI matching web ConfirmDialog
