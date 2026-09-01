# CompanyOS Mobile (Flutter) — Phase 1.11

High-frequency mobile shell (not full web parity):

- Auth + org switch via existing CompanyOS APIs (`LOCAL_AUTH` in CI)
- Bottom tabs: **Home · Work · Create · Inbox · More**
- Approvals, tasks, deal quick-updates, camera-first expense capture
- Offline read cache + mutation queue with stable `Idempotency-Key`
- Push + biometric **interfaces** with `FakePushService` / `FakeBiometricService` in CI
- Deep links: `companyos://record/{id}?org=org_…`

## CI

```bash
cd apps/mobile
flutter pub get
flutter analyze
flutter test
# optional: flutter build apk --debug  (unsigned; store signing is a follow-up)
```

Do **not** require live FCM/APNs keys, real device biometrics, or Xcode-signed iOS.

## Config

- `API_URL` dart-define (default `http://127.0.0.1:8080`)
