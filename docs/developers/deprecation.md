# Deprecation policy

Within a major version CompanyOS may deprecate a field or path with:

1. **Dual-publish** — old and new names both present
2. **`Deprecation: true`** response header
3. **`Sunset:`** HTTP-date (~180 days out)
4. **`Link:`** to this policy / migration notes

After Sunset the deprecated name may be removed in a subsequent minor release of
the same major, or held until the next major.

## Exercise (Phase 3.3)

`ApiKeyExchangeResponse.rate_limit_rpm` is a deprecated alias of
`rate_limit_per_minute`. The exchange endpoint returns both and sets Deprecation /
Sunset / Link headers. CI freezes the previous public OpenAPI snapshot and asserts
the dual-publish + deprecated annotation remain until Sunset.
