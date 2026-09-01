# Secret rotation (test + production outline)

## Scope

- **CI / test:** rotate a MockKms wrap key via `scripts/ops/rotate-test-secret.sh`
  (never touches production).
- **Auth JWKS:** [auth-key-rotation.md](./auth-key-rotation.md)
- **CMEK (enterprise):** MockKms in CI; real AWS KMS is out of scope for this pack

## CI hook

```bash
./scripts/ops/rotate-test-secret.sh
# wraps: cargo test -p companyos-crypto mock_kms_rotate_and_revoke -- --exact
```

## Production outline (human)

1. Schedule rotation window; notify owners
2. Rotate JWKS via `POST /api/v1/auth/jwks/rotate`
3. For CMEK tenants: provision new CMK, re-wrap org DEK, revoke old after verify
4. Rotate gateway/service env secrets via secret manager (not in-repo)
5. Confirm login + encrypted field read still succeed
6. Record evidence in SOC 2 binder (change ticket id)

## Never

- Commit production secrets
- Set live `AI_API_KEY` in CI
- Rotate customer CMK without dual-control
