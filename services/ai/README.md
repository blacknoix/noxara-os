# AI service group (Phase 1.9)

Copilot orchestration, tenant-filtered retrieval, and propose-then-commit write
previews. AI is a first-class caller of the same gateway APIs and
`crates/authz` policies as humans (invariant 2, ADR 012). **No privileged
bypass** — every tool call uses the invoking user's authority.

## Binary

`companyos-ai` (bind `AI_BIND`, default `:8092`) — schema `ai_*` tables with RLS.

## Local model key

```bash
export AI_API_KEY=sk-...          # enables OpenAI-compatible provider
export AI_API_BASE=https://api.openai.com/v1   # optional
export AI_MODEL=gpt-4o-mini                    # optional
```

Without `AI_API_KEY`, the service uses the **mock provider** (fixtures). CI and
tests never require a live key.

## Principles (v1)

1. Participant, not backdoor — acts as the invoking user.
2. Propose, then commit — writes return proposals until confirm.
3. Always cite — factual claims link to retrieved records.
4. Reversible by default — AI-originated changes tagged in proposals.
5. `crates/authz` before every tool; decision recorded in the tool trace.
6. Tenant isolation in retrieval — `org_id` required at query construction.
7. Untrusted content delimited; never granted tool authority.
8. Money stays integer minor units.
