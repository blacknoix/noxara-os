# Public scopes

API key scopes must be permission IDs from this catalogue:

| Scope | Use |
|-------|-----|
| `sales.customer.read` / `.create` / `.update` | Customers |
| `sales.deal.read` / `.create` / `.update` | Deals |
| `sales.quote.read` / `.create` / `.update` | Quotes |
| `finance.invoice.read` / `.create` / `.update` / `.issue` / `.send` | Invoices |
| `finance.payment.read` / `.create` | Payments |
| `admin.api_key.manage` | Manage keys (human admin flows) |
| `admin.webhook.read` / `.write` / `.replay` | Outbound webhooks |

Unknown scopes are rejected at key creation (`400`).
Effective permissions = **key scopes ∩ owner role allows**.
