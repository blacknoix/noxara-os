# Threat model — Authentication surface (Phase 1.1)

## Assets

- User credentials (password hashes, TOTP secrets, recovery codes)
- Org-scoped access JWTs and opaque refresh tokens
- Session / device binding metadata
- Membership and role assignments (`policy_version`)
- Signing keys (JWKS / HS256 secrets)

## Trust boundaries

| Boundary | Notes |
|----------|-------|
| Browser ↔ Gateway | TLS in prod; refresh cookie `httpOnly` + `Secure` + `SameSite=Lax` |
| Gateway ↔ Core | Private network; gateway verifies JWT + coarse authz via `crates/authz` |
| Core ↔ Postgres | RLS with `app.org_id`; global `user_identity` is not tenant-RLS |
| OAuth IdPs | Google / Microsoft; credentials from env; PKCE |

## Key threats & controls

1. **Credential stuffing / brute force** — per-endpoint rate limits, progressive delays, account lockout after repeated failures (tested). Responses are 401/403/429, never 500.
2. **Password breach reuse** — Argon2id + per-user salt; HIBP k-anonymity when `HIBP_ENABLED=1`, fixture list otherwise.
3. **Refresh token theft / replay** — opaque tokens hashed at rest; rotation on each use; **reuse of a rotated token revokes the entire family**.
4. **Cross-tenant access** — `org_id` claim mandatory on access tokens; live membership + session revocation checks on every authenticated request; RLS on tenant tables.
5. **Org switch confusion** — switching orgs is `POST /auth/switch-org` minting a **new** access token (new `org_id` + `policy_version`), never a client header swap.
6. **Privilege escalation** — `crates/authz` is the sole PDP; Owner/Admin require MFA before access token issuance.
7. **Session fixation / mass compromise** — list/revoke one/all sessions; password reset revokes all sessions.
8. **SSO misconfig** — SAML/OIDC config stored but **disabled** unless plan flag + `COMPANYOS_SSO_ENABLED`; admin API returns `feature_disabled` (403).
9. **LOCAL-ONLY bypass abuse** — `COMPANYOS_LOCAL_AUTH` defaults **off**; clearly logged when used.
10. **No god tokens** — deny by default; no service-account superuser JWT in Phase 1.1.

## Out of scope (1.1)

Full IdP implementation, hardware WebAuthn, step-up for every sensitive finance action (later phases).
