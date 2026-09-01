# Agent kill switch runbook (Phase 4.3)

## What it does

Engaging the org-wide kill switch (`POST /api/v1/ai/agents/kill-switch` with
`engaged: true`, `agent_type: "*"`) immediately:

1. Refuses new agent runs for the org
2. Marks in-flight `ai_agent_run` rows as `killed`
3. Causes cooperative agent loops to stop before the next tool call
4. Updates the in-process kill-switch cache so same-pod agents observe the halt
   within milliseconds

Optional `agent_type` scopes the halt to one agent (e.g. `receivables_chase`).

Requires `ai.agent.kill` (Owner/Admin). Members cannot flip it.

## Production mapping

CI asserts halt within **≤ 2 seconds** (tight bound using `step_delay_ms` +
50 ms poll). In production, map that to:

| Layer | Bound |
| --- | --- |
| Same pod (cache) | ≤ 100 ms |
| Cross replica (Postgres read on next tool) | ≤ 2 s typical; runbook target **≤ 5 s** |
| Temporal `AgentRun` workflows | Paused/cancelled via status + catalogue `signal_kill` |

## How to engage

1. Settings → AI → Agents → Kill switch, or
2. `POST /api/v1/ai/agents/kill-switch` with bearer token of Owner/Admin

## How to clear

Same endpoint with `engaged: false`. New runs resume only if policy + budget
still allow them.

## Related

- ADR-012 (governed exception to propose-then-commit)
- `services/ai/ai-service/src/agents/kill_switch.rs`
- Temporal id: `{org}:AgentRun:{run_public_id}`
