# Store release — CompanyOS mobile clients

How to produce and (later) submit store artifacts. **This document describes the
pipeline; it is not a submit checklist that has been executed.**

## What this PR / pipeline does

| Artifact | CI job | Status |
| --- | --- | --- |
| Flutter analyze + unit tests | `flutter` | Required (unchanged) |
| Unsigned debug APK | `flutter` (best-effort step) | `continue-on-error` (unchanged) |
| **Signed Android App Bundle** | `android-signed-release` | **Required** — CI-only upload keystore generated in-job and discarded |
| iOS Fastlane dry-run | `ios-fastlane-dry-run` | Required on Linux — validates lanes/files; **no IPA** |
| iOS signed IPA (`gym` / `match`) | `ios-release` | Skipped on Linux (`if: false`) — needs macOS + Apple certs |

Push remains **fakes** in CI (`FakePushService`). Crash reporting uses a **fake DSN /
no-op transport** (`FakeCrashTransport`). No live FCM, APNs, Sentry project, or
store upload.

## What this PR does **not** do

- `fastlane deliver` / `supply` / Play Console or App Store Connect upload
- Commit a production Play upload key, Apple `.p12`, or API keys
- Live `AI_API_KEY`, FCM, APNs, or Sentry DSN
- Tauri notarization / Mac App Store
- Start live AWS, real KMS, or a new product module

## Android signed release (required CI)

1. Job generates an ephemeral PKCS12 keystore via
   `apps/mobile/scripts/ci_android_keystore.sh` (90-day CI-only key).
2. Env vars `COMPANYOS_UPLOAD_STORE_FILE`, `COMPANYOS_UPLOAD_STORE_PASSWORD`,
   `COMPANYOS_UPLOAD_KEY_ALIAS`, `COMPANYOS_UPLOAD_KEY_PASSWORD` configure
   `android/app/build.gradle.kts` `signingConfigs.upload`.
3. `flutter build appbundle --release` with `--build-name` / `--build-number`
   from `scripts/version_from_git.sh` (pubspec name + `git rev-list --count`).
4. Artifact uploaded as `android-signed-aab` (and optional APK).
5. Keystore is **not** cached and **not** committed.

### Play upload key (owner follow-up)

The CI keystore is **not** the production Play App Signing / upload key.
When ready to submit:

1. Create a real upload keystore offline (or in a secure secrets manager).
2. Register it with Play App Signing.
3. Store as GitHub Actions secrets, for example:
   - `PLAY_UPLOAD_KEYSTORE_BASE64`
   - `PLAY_UPLOAD_STORE_PASSWORD`
   - `PLAY_UPLOAD_KEY_ALIAS`
   - `PLAY_UPLOAD_KEY_PASSWORD`
4. Point a future (non-CI-ephemeral) release workflow at those secrets.
5. Optionally use `fastlane supply` with a Play service account JSON secret —
   **not** wired in this PR.

## iOS Fastlane (Mac runner later)

Lanes live under `apps/mobile/ios/fastlane/`:

| Lane | Purpose |
| --- | --- |
| `ios dry_run` | File + Info.plist checks; no Xcode |
| `ios beta` | `match` (appstore) + `flutter build ipa` + export options |
| `ios release` | Same as beta; **no** `deliver` |

### Required later (not available on Linux CI)

- macOS GitHub-hosted or self-hosted runner with Xcode
- Apple Developer Program membership
- `match` certificate storage (`MATCH_GIT_URL`, `MATCH_PASSWORD`)
- App Store Connect API key or Apple ID (`APPLE_ID`, `APPLE_TEAM_ID`,
  `ITC_TEAM_ID`, `APP_STORE_CONNECT_API_KEY_*`)
- Update `ExportOptions.plist` team ID + provisioning profile names

Linux job `ios-release` is disabled (`if: false`) so missing Xcode never fails
the PR. `ios-fastlane-dry-run` prints a clear skip note for gym/IPA.

## Crash reporting

- Interface: `apps/mobile/lib/src/crash/crash_reporter.dart`
- CI / default: empty or `fake` / `noop` `CRASH_DSN` → `FakeCrashTransport`
- Tests assert initialize + org/PII redaction
- Live Sentry (or similar) = implement `CrashTransport` and pass a real DSN via
  `--dart-define=CRASH_DSN=...` — **not required for CI green**

## Push / biometrics

Unchanged from Phase 1.11: `FakePushService` / `FakeBiometricService` in CI.
Do not invent FCM/APNs secrets for this pipeline.

## Store listing stubs

`apps/mobile/metadata/{android,ios}/en-US/` — title/description stubs only.
Not submitted.

## Versioning

```bash
cd apps/mobile
eval "$(./scripts/version_from_git.sh)"
# COMPANYOS_BUILD_NAME from pubspec (e.g. 0.1.0)
# COMPANYOS_BUILD_NUMBER from git rev-list --count HEAD
flutter build appbundle --release \
  --build-name="$COMPANYOS_BUILD_NAME" \
  --build-number="$COMPANYOS_BUILD_NUMBER"
```

## Local dry checks

```bash
# Crash + push fakes
cd apps/mobile && flutter test

# Ephemeral keystore smoke (does not commit)
./scripts/ci_android_keystore.sh /tmp/cos-ks

# iOS lane dry-run (Ruby + bundler; no Xcode)
cd ios && bundle install && bundle exec fastlane ios dry_run
```
