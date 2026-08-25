# Runbook: Locked-out owner recovery

## Symptoms

Owner cannot sign in: `account_locked`, lost MFA device, or exhausted recovery codes.

## Progressive lockout (password failures)

1. Wait for `locked_until` to pass (default 15 minutes), **or** clear via break-glass SQL:

```sql
UPDATE user_identity
SET failed_login_count = 0, locked_until = NULL
WHERE email_normalized = lower('owner@example.com');
```

2. Prefer password reset email (`POST /api/v1/auth/password-reset/request`) which also revokes sessions after confirm.

## Lost MFA device

1. If recovery codes remain: sign in → MFA challenge → submit `recovery_code`.
2. If no recovery codes and no other Owner/Admin:

```sql
-- Break-glass: disable MFA and force re-enrollment (audit this!)
UPDATE user_identity
SET mfa_enabled_at = NULL, mfa_totp_secret_encrypted = NULL
WHERE id = '<user-uuid>';
DELETE FROM mfa_recovery_code WHERE user_id = '<user-uuid>';
```

3. Owner signs in, hits MFA-required setup path, enrolls a new authenticator, stores new recovery codes.

## No email access

Coordinate out-of-band identity proof with the customer; update `email` / `email_normalized` only after verification; issue a one-time password reset token into `email_token` (purpose `password_reset`) and deliver securely.

## Always

Write an `auth_audit_event` note (or ticket link) for any break-glass SQL.
