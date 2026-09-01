# AI spend anomaly

## Meaning

An org’s AI token usage or estimated cost spikes, or monthly budget hard-stop
trips. Agents may refuse new runs; copilot should fail closed when the provider
is unavailable.

## Check

```sql
-- Usage ledger patterns (schema evolves with ai-service migrations)
SELECT org_id, date_trunc('day', created_at) AS day, COUNT(*), SUM(cost_estimate_minor)
FROM ai_usage_event
GROUP BY 1, 2
ORDER BY 2 DESC
LIMIT 30;
```

Confirm kill switch / budget:

```bash
curl -s "$GATEWAY_URL/api/v1/ai/agents/kill-switch" -H "Authorization: Bearer $TOKEN"
```

## Remediation

1. Engage agent kill switch if autonomous spend is the source ([agent-kill-switch](./agent-kill-switch.md))
2. Disable copilot module in AI settings if chat is the burn source
3. Verify no live `AI_API_KEY` leakage in logs (forbidden)
4. Provider down → copilot returns `feature_disabled`; rest of app continues
