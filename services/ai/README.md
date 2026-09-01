# AI service group (Phase 1.9 + 3.5 + 4.3)

Copilot orchestration, tenant-filtered retrieval, propose-then-commit write
previews, and **Phase 4.3 governed autonomous agents**. AI is a first-class
caller of the same gateway APIs and `crates/authz` policies as humans
(invariant 2, ADR 012). **No privileged bypass** — every tool call uses the
invoking user's authority (or a policy ∩ on_behalf_of intersection for agents).

## Binary

`companyos-ai` (bind `AI_BIND`, default `:8092`) — schema `ai_*` tables with RLS.

## Local model key

```bash
export AI_API_KEY=sk-...          # enables OpenAI-compatible provider
export AI_API_BASE=https://api.openai.com/v1   # optional
export AI_MODEL=gpt-4o-mini                    # optional
```

Without `AI_API_KEY`, the service uses the **mock provider** (fixtures). CI and
tests never require a live key. Agent runs honor the tenant prompt-pack routing
profile (allowed models / temperature / tool subset) — **no real fine-tunes in 4.3**.

## Principles

1. Participant, not backdoor — acts as the invoking user / policy intersection.
2. Propose, then commit — **default** for copilot writes until confirm.
3. Governed exception (4.3) — unattended writes only inside a declared agent policy.
4. Always cite — factual claims link to retrieved records.
5. Reversible by default — AI-originated changes tagged in proposals / `ai_action`.
6. `crates/authz` before every tool; decision recorded in the tool trace.
7. Tenant isolation in retrieval — `org_id` required at query construction.
8. Untrusted content delimited; never granted tool authority.
9. Money stays integer minor units.
10. Org kill switch + monthly token budget hard-stop for agents.

## Phase 4.3 routes

| Path | Purpose |
| --- | --- |
| `/api/v1/ai/agents/policy` | Versioned org agent policy |
| `/api/v1/ai/agents/kill-switch` | Org / per-agent halt |
| `/api/v1/ai/agents/runs` | Start / list agent runs |
| `/api/v1/ai/agents/actions/{id}/reverse` | Undo `ai_action` as a unit |
| `/api/v1/ai/agents/workflows/propose` | NL → 3.1 workflow **draft** |
| `/api/v1/ai/agents/prompt-pack` | Tenant routing profile |
| `/api/v1/ai/agents/review` | Quarterly review report |

See `docs/runbooks/agent-kill-switch.md` and ADR-012.
