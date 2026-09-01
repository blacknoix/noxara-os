'use client';

import { useCallback, useEffect, useState, type CSSProperties, type FormEvent } from 'react';
import Link from 'next/link';
import {
  Button,
  EmptyState,
  ErrorState,
  InlineAlert,
  Input,
  PermissionDeniedState,
  Switch,
  Textarea,
} from '@companyos/design-system';
import {
  fetchAgentPolicy,
  fetchAgentReview,
  fetchAgentRuns,
  fetchKillSwitch,
  fetchPromptPack,
  proposeNlWorkflow,
  publishAgentPolicy,
  savePromptPack,
  setKillSwitch,
  startAgentRun,
} from '../../../../lib/agent-api';
import type {
  AgentPolicyDoc,
  AgentReviewReport,
  AgentRunView,
  KillSwitchView,
  NlWorkflowDraft,
  PromptPackDoc,
} from '../../../../lib/agent-types';
import { getAccessToken } from '../../../../lib/auth-client';
import { useCapabilities } from '../../../../lib/capabilities';

const DEFAULT_POLICY: AgentPolicyDoc = {
  name: 'default',
  agent_types: ['receivables_chase'],
  allowed_tools: ['list_overdue_invoices', 'send_invoice_reminder', 'escalate_exception'],
  allowed_permissions: [
    'finance.invoice.read',
    'finance.invoice.send',
    'platform.notification.read',
    'operations.task.create',
  ],
  spend_budget_tokens: 100000,
  max_steps: 50,
  require_human_above: {
    permissions: ['finance.invoice.void', 'finance.journal.post', 'hr.payroll.run'],
    amount_minor: 1000000,
  },
  allowed_resource_scopes: ['finance.invoices', 'notifications'],
};

export default function AgentsSettingsPage() {
  const { can, loading: capsLoading } = useCapabilities();
  const canRead = can('ai.agent.read');
  const canManage = can('ai.agent.manage');
  const canRun = can('ai.agent.run');
  const canKill = can('ai.agent.kill');

  const [policy, setPolicy] = useState<AgentPolicyDoc>(DEFAULT_POLICY);
  const [policyVersion, setPolicyVersion] = useState<number | null>(null);
  const [kill, setKill] = useState<KillSwitchView | null>(null);
  const [runs, setRuns] = useState<AgentRunView[]>([]);
  const [pack, setPack] = useState<PromptPackDoc | null>(null);
  const [nlPrompt, setNlPrompt] = useState('');
  const [nlDraft, setNlDraft] = useState<NlWorkflowDraft | null>(null);
  const [review, setReview] = useState<AgentReviewReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    if (!getAccessToken()) return;
    const [p, k, r, pk, rev] = await Promise.all([
      fetchAgentPolicy(),
      fetchKillSwitch(),
      fetchAgentRuns(),
      fetchPromptPack(),
      fetchAgentReview(),
    ]);
    if (p) {
      setPolicy(p.policy);
      setPolicyVersion(p.version);
    }
    setKill(k);
    setRuns(r);
    setPack(pk);
    setReview(rev);
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function onPublishPolicy(e: FormEvent) {
    e.preventDefault();
    if (!canManage) return;
    setBusy(true);
    setError(null);
    const saved = await publishAgentPolicy(policy);
    setBusy(false);
    if (!saved) {
      setError('Could not publish agent policy.');
      return;
    }
    setPolicy(saved.policy);
    setPolicyVersion(saved.version);
  }

  async function onToggleKill(engaged: boolean) {
    if (!canKill) return;
    setBusy(true);
    setError(null);
    const view = await setKillSwitch({
      engaged,
      agent_type: '*',
      reason: engaged ? 'Engaged from Settings' : undefined,
    });
    setBusy(false);
    if (!view) {
      setError('Kill switch update failed.');
      return;
    }
    setKill(view);
    await load();
  }

  async function onRunChase() {
    if (!canRun) return;
    setBusy(true);
    setError(null);
    const outcome = await startAgentRun('receivables_chase');
    setBusy(false);
    if (!outcome) {
      setError('Agent run failed (policy, budget, or kill switch).');
      return;
    }
    await load();
  }

  async function onProposeNl(e: FormEvent) {
    e.preventDefault();
    if (!canRun || !nlPrompt.trim()) return;
    setBusy(true);
    setError(null);
    const draft = await proposeNlWorkflow(nlPrompt.trim());
    setBusy(false);
    if (!draft) {
      setError('NL workflow proposal failed.');
      return;
    }
    setNlDraft(draft);
  }

  async function onSavePack(e: FormEvent) {
    e.preventDefault();
    if (!canManage || !pack) return;
    setBusy(true);
    const saved = await savePromptPack(pack);
    setBusy(false);
    if (!saved) {
      setError('Could not save prompt pack.');
      return;
    }
    setPack(saved);
  }

  if (!getAccessToken()) {
    return <ErrorState title="Sign in required" message="Open /login to continue." />;
  }
  if (capsLoading) {
    return <p style={{ color: 'var(--cos-color-fg-muted)' }}>Loading…</p>;
  }
  if (!canRead && !canManage) {
    return <PermissionDeniedState requiredPermission="ai.agent.read" />;
  }

  return (
    <div style={{ display: 'grid', gap: '1.5rem', maxWidth: 720 }}>
      <header>
        <p style={eyebrow}>Settings</p>
        <h1 style={h1}>Agents</h1>
        <p style={muted}>
          Governed autonomous agents — scoped, budgeted, reversible, audited.{' '}
          <Link href="/settings/ai" style={{ color: 'var(--cos-color-accent)' }}>
            AI settings
          </Link>
          {' · '}
          <Link href="/workflows" style={{ color: 'var(--cos-color-accent)' }}>
            Workflows
          </Link>
        </p>
      </header>

      {error ? <ErrorState message={error} /> : null}
      {kill?.engaged ? (
        <InlineAlert tone="warning" title="Kill switch engaged">
          All agent activity for this organization is halted.
        </InlineAlert>
      ) : null}

      <section aria-labelledby="kill-heading" style={section}>
        <h2 id="kill-heading" style={h2}>
          Kill switch
        </h2>
        <p style={muted}>Halts new tool calls and in-flight agent runs within seconds.</p>
        <Switch
          label="Org-wide kill switch"
          description="Owner/Admin only"
          checked={Boolean(kill?.engaged)}
          disabled={!canKill || busy}
          onCheckedChange={(v) => void onToggleKill(v)}
        />
      </section>

      <section aria-labelledby="policy-heading" style={section}>
        <h2 id="policy-heading" style={h2}>
          Agent policy
        </h2>
        <p style={muted}>
          Versioned allow-list. In-flight runs keep the version they started.
          {policyVersion != null ? ` Active version: ${policyVersion}.` : ' No active policy yet.'}
        </p>
        <form onSubmit={(e) => void onPublishPolicy(e)} style={{ display: 'grid', gap: '0.75rem' }}>
          <Input
            label="Name"
            name="policy-name"
            value={policy.name}
            disabled={!canManage}
            onChange={(e) => setPolicy({ ...policy, name: e.target.value })}
          />
          <Input
            label="Allowed tools (comma-separated)"
            name="allowed-tools"
            value={policy.allowed_tools.join(', ')}
            disabled={!canManage}
            onChange={(e) =>
              setPolicy({
                ...policy,
                allowed_tools: e.target.value
                  .split(',')
                  .map((s) => s.trim())
                  .filter(Boolean),
              })
            }
          />
          <Input
            label="Allowed permissions (comma-separated)"
            name="allowed-perms"
            value={policy.allowed_permissions.join(', ')}
            disabled={!canManage}
            onChange={(e) =>
              setPolicy({
                ...policy,
                allowed_permissions: e.target.value
                  .split(',')
                  .map((s) => s.trim())
                  .filter(Boolean),
              })
            }
          />
          <Input
            label="Max steps"
            name="max-steps"
            type="number"
            value={String(policy.max_steps)}
            disabled={!canManage}
            onChange={(e) =>
              setPolicy({ ...policy, max_steps: Number(e.target.value) || policy.max_steps })
            }
          />
          {canManage ? (
            <Button type="submit" disabled={busy}>
              Publish policy version
            </Button>
          ) : null}
        </form>
      </section>

      <section aria-labelledby="monitor-heading" style={section}>
        <h2 id="monitor-heading" style={h2}>
          Monitor
        </h2>
        <p style={muted}>Running / waiting / failed / killed — last actions and cost.</p>
        {canRun ? (
          <Button type="button" disabled={busy || Boolean(kill?.engaged)} onClick={() => void onRunChase()}>
            Run receivables chase
          </Button>
        ) : null}
        {runs.length === 0 ? (
          <EmptyState title="No agent runs yet" description="Publish a policy, then start a run." />
        ) : (
          <ul style={list}>
            {runs.map((r) => (
              <li key={r.id} style={listItem}>
                <strong>{r.agent_type}</strong> — {r.status} · steps {r.steps_taken} · tokens{' '}
                {r.tokens_used} · {r.public_id}
              </li>
            ))}
          </ul>
        )}
      </section>

      <section aria-labelledby="nl-heading" style={section}>
        <h2 id="nl-heading" style={h2}>
          Natural-language workflow
        </h2>
        <p style={muted}>
          Proposes a draft using the 3.1 catalogue only. You publish in Workflows — AI cannot publish.
        </p>
        <form onSubmit={(e) => void onProposeNl(e)} style={{ display: 'grid', gap: '0.75rem' }}>
          <Textarea
            label="Describe the workflow"
            name="nl-prompt"
            value={nlPrompt}
            disabled={!canRun}
            onChange={(e) => setNlPrompt(e.target.value)}
          />
          {canRun ? (
            <Button type="submit" disabled={busy || !nlPrompt.trim()}>
              Propose draft
            </Button>
          ) : null}
        </form>
        {nlDraft ? (
          <div style={{ marginTop: '0.75rem' }}>
            <p style={muted}>{nlDraft.note}</p>
            {nlDraft.filtered_actions.length > 0 ? (
              <InlineAlert tone="warning" title="Filtered actions">
                {nlDraft.filtered_actions.join('; ')}
              </InlineAlert>
            ) : null}
            <pre style={pre}>{JSON.stringify(nlDraft.definition, null, 2)}</pre>
            <Link href="/workflows" style={{ color: 'var(--cos-color-accent)' }}>
              Open workflow builder
            </Link>
          </div>
        ) : null}
      </section>

      <section aria-labelledby="pack-heading" style={section}>
        <h2 id="pack-heading" style={h2}>
          Prompt pack (routing profile)
        </h2>
        <p style={muted}>
          Allowed models, temperature, and tool subset. Real fine-tunes are later — not in this release.
        </p>
        {pack ? (
          <form onSubmit={(e) => void onSavePack(e)} style={{ display: 'grid', gap: '0.75rem' }}>
            <Input
              label="Allowed models"
              name="models"
              value={pack.allowed_models.join(', ')}
              disabled={!canManage}
              onChange={(e) =>
                setPack({
                  ...pack,
                  allowed_models: e.target.value
                    .split(',')
                    .map((s) => s.trim())
                    .filter(Boolean),
                })
              }
            />
            <Textarea
              label="System preamble"
              name="preamble"
              value={pack.system_preamble}
              disabled={!canManage}
              onChange={(e) => setPack({ ...pack, system_preamble: e.target.value })}
            />
            {canManage ? (
              <Button type="submit" disabled={busy}>
                Save prompt pack
              </Button>
            ) : null}
          </form>
        ) : (
          <p style={muted}>Loading prompt pack…</p>
        )}
      </section>

      <section aria-labelledby="review-heading" style={section}>
        <h2 id="review-heading" style={h2}>
          Review pack
        </h2>
        {review ? (
          <p style={muted}>
            Actions {review.total_actions} · failures {review.failures} · reversals {review.reversals}{' '}
            · error rate {(review.error_rate * 100).toFixed(1)}% (threshold{' '}
            {(review.max_error_rate * 100).toFixed(1)}%) —{' '}
            {review.within_threshold ? 'within threshold' : 'above threshold'}
          </p>
        ) : (
          <p style={muted}>No review data yet.</p>
        )}
      </section>
    </div>
  );
}

const eyebrow: CSSProperties = {
  margin: 0,
  fontSize: '0.75rem',
  letterSpacing: '0.06em',
  textTransform: 'uppercase',
  color: 'var(--cos-color-fg-muted)',
};
const h1: CSSProperties = { margin: '0.25rem 0', fontSize: '1.75rem', fontWeight: 600 };
const h2: CSSProperties = { margin: 0, fontSize: '1.1rem', fontWeight: 600 };
const muted: CSSProperties = { margin: '0.25rem 0 0', color: 'var(--cos-color-fg-muted)', fontSize: '0.9rem' };
const section: CSSProperties = {
  display: 'grid',
  gap: '0.75rem',
  paddingTop: '0.5rem',
  borderTop: '1px solid var(--cos-color-border)',
};
const list: CSSProperties = { margin: 0, paddingLeft: '1.25rem', display: 'grid', gap: '0.35rem' };
const listItem: CSSProperties = { fontSize: '0.9rem' };
const pre: CSSProperties = {
  margin: '0.5rem 0',
  padding: '0.75rem',
  overflow: 'auto',
  fontSize: '0.8rem',
  background: 'var(--cos-color-bg-subtle, #f6f6f4)',
  borderRadius: 4,
};
