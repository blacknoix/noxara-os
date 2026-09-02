# CompanyOS Mobile (Flutter) — Phase 1.11 + store pipeline

High-frequency mobile shell (not full web parity):

- Auth + org switch via existing CompanyOS APIs (`LOCAL_AUTH` in CI)
- Bottom tabs: **Home · Work · Create · Inbox · More**
- Approvals, tasks, deal quick-updates, camera-first expense capture
- Offline read cache + mutation queue with stable `Idempotency-Key`
- Push + biometric **interfaces** with `FakePushService` / `FakeBiometricService` in CI
- Crash reporting behind config (`CrashReporter` + `FakeCrashTransport`; fake DSN in CI)
- Deep links: `companyos://record/{id}?org=org_…`
- Store pipeline: signed Android AAB in CI; iOS Fastlane dry-run on Linux

## CI

```bash
cd apps/mobile
flutter pub get
flutter analyze
flutter test
# Required separate job: android-signed-release (ephemeral CI keystore → AAB)
# iOS: bundle exec fastlane ios dry_run  (no Xcode / no IPA on Linux)
```

Do **not** require live FCM/APNs keys, real device biometrics, live Sentry DSN,
or Xcode-signed iOS for PR green. See `docs/clients/store-release.md`.

## Config

- `API_URL` dart-define (default `http://127.0.0.1:8080`)
- `CRASH_DSN` dart-define — empty / `fake` / `noop` → no-op transport (CI default)
