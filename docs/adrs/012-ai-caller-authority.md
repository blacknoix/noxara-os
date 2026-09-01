# ADR 012: AI caller authority; propose-then-commit (with governed agent exception)

- Status: **Accepted** (amended Phase 4.3)
- Date: 2026-03-25
- Amended: 2026-09-01 (Phase 4.3)

## Context

CompanyOS Phase 0 locked foundational platform decisions before product domains landed.
Until Phase 4.3, v1 AI writes were previewed diffs — tagged, cited, reversible — and
required human commit.

## Decision

AI calls the same APIs and `authz` policies as humans. **Default remains propose-then-commit.**

Phase 4.3 introduces the **first governed exception**: unattended writes are allowed
**only** inside a declared org agent policy. There is still no AI bypass of
`crates/authz`. The AI service holds **no independent authority**.

### Agent principal

The agent is a documented machine principal with `on_behalf_of` a human (or a
scheduled **policy**, not a superuser). Effective permissions =

```text
policy allow-list ∩ on_behalf_of ∩ org roles
```

Narrower wins. Every tool maps to a permission; `check()` / `decide()` before
execution; decision recorded in the tool trace and on `ai_action`.

### Kill switch & budget

An org-wide (optional per-agent) kill switch halts new tool calls and in-flight
agent runs within seconds. Agents consume the existing per-org monthly token
budget and hard-stop when exhausted.

### Reversibility

Every autonomous write links to `ai_action` with model, prompt version, tool
trace, and reversibility metadata. Undo as a unit within the reversibility window.

## Consequences

- Implementations that violate this ADR are rejected in review.
- Related invariants: see `docs/00-INDEX.md`.
- Copilot chat without an authorizing agent policy still proposes only.
