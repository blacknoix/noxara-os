# ADR 016: Org-scoped JWT access + opaque refresh cookies

- Status: **Accepted**
- Date: 2026-08-25

## Context

Phase 1.1 must replace Phase 0 LOCAL-ONLY header/unsigned JWT stubs with production-shaped authentication while preserving RLS tenancy and `crates/authz` as the sole PDP.

## Decision

1. **Access tokens** are short-lived **JWTs (HS256)** that always include `org_id`, `membership_id`, `roles`, `policy_version`, and `sid`. Verification uses a rotating keyring exposed at `/api/v1/auth/jwks.json`.
2. **Refresh tokens** are **opaque** random values stored only as SHA-256 hashes. They are delivered in an **httpOnly Secure SameSite=Lax** cookie (`companyos_refresh`), never in `localStorage`.
3. **Rotation + reuse detection**: each refresh marks the prior token rotated; presenting a rotated token revokes the entire token family.
4. **Org switching** is `POST /api/v1/auth/switch-org`, which mints a **new** access token for the target membership — clients must not swap org via headers.
5. **LOCAL-ONLY** Phase 0 bypass remains behind `COMPANYOS_LOCAL_AUTH` (default **off**) for tests/scripts only.
6. Access tokens are held in **web memory** (module variable), not `localStorage`.

## Consequences

- Gateway and core share JWT verification material; gateway performs coarse authz pre-checks via `crates/authz`.
- Membership revocation + session revoke are checked live on authenticated requests (invalid within seconds).
- HS256 JWKS is unconventional vs asymmetric keys; acceptable for Phase 1.1 co-deployed core/gateway; migrate to asymmetric kids if services fan out further.
