'use client';

import { useCallback, useEffect, useState, type CSSProperties, type FormEvent } from 'react';
import Link from 'next/link';
import {
  Button,
  Checkbox,
  EmptyState,
  ErrorState,
  InlineAlert,
  Input,
  PermissionDeniedState,
  Select,
  Switch,
} from '@companyos/design-system';
import { fetchAiSettings, patchAiSettings } from '../../../lib/ai-api';
import type { AiSettings } from '../../../lib/ai-types';
import { getAccessToken } from '../../../lib/auth-client';
import { useCapabilities } from '../../../lib/capabilities';

const MODEL_OPTIONS = [
  { value: 'mock', label: 'Mock (development)' },
  { value: 'gpt-4o-mini', label: 'GPT-4o mini' },
  { value: 'gpt-4o', label: 'GPT-4o' },
];

export default function AiSettingsPage() {
  const { can, loading: capsLoading } = useCapabilities();
  const [settings, setSettings] = useState<AiSettings | null>(null);
  const [allowListText, setAllowListText] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const canRead = can('ai.settings.read');
  const canManage = can('ai.settings.manage');

  const load = useCallback(async () => {
    if (!getAccessToken()) return;
    const data = await fetchAiSettings();
    if (!data) {
      setError('Could not load AI settings.');
      return;
    }
    setSettings(data);
    setAllowListText(data.auto_execute_allow_list.join(', '));
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function onSave(e: FormEvent) {
    e.preventDefault();
    if (!settings || !canManage) return;
    setBusy(true);
    setError(null);
    const allowList = allowListText
      .split(',')
      .map((s) => s.trim())
      .filter(Boolean);
    const updated = await patchAiSettings({
      modules_enabled: settings.modules_enabled,
      model_preference: settings.model_preference,
      auto_execute_allow_list: allowList,
      data_sharing: settings.data_sharing,
      monthly_token_budget: settings.monthly_token_budget,
    });
    setBusy(false);
    if (!updated) {
      setError('Save failed.');
      return;
    }
    setSettings(updated);
    setAllowListText(updated.auto_execute_allow_list.join(', '));
  }

  if (!getAccessToken()) {
    return <ErrorState title="Sign in required" message="Open /login to continue." />;
  }

  if (capsLoading) {
    return <p style={{ color: 'var(--cos-color-fg-muted)' }}>Loading…</p>;
  }

  if (!canRead && !canManage) {
    return <PermissionDeniedState requiredPermission="ai.settings.read" />;
  }

  if (!settings) {
    return error ? <ErrorState message={error} /> : <p style={{ color: 'var(--cos-color-fg-muted)' }}>Loading AI settings…</p>;
  }

  const budgetPct =
    settings.monthly_token_budget > 0
      ? Math.min(100, Math.round((settings.tokens_used_this_month / settings.monthly_token_budget) * 100))
      : 0;

  return (
    <div style={{ display: 'grid', gap: '1.25rem', maxWidth: 560 }}>
      <header>
        <p style={eyebrow}>Settings</p>
        <h1 style={h1}>AI</h1>
        <p style={muted}>
          Copilot modules, model preference, and data sharing for your organization.{' '}
          <Link href="/settings" style={{ color: 'var(--cos-color-accent)' }}>
            Back to settings
          </Link>
          {' · '}
          <Link href="/settings/ai/meeting-summaries" style={{ color: 'var(--cos-color-accent)' }}>
            Meeting summaries
          </Link>
        </p>
      </header>

      {error ? <ErrorState message={error} /> : null}

      <form onSubmit={(e) => void onSave(e)} style={{ display: 'grid', gap: '1rem' }}>
        <fieldset style={fieldset}>
          <legend style={legend}>Modules</legend>
          <Switch
            label="Copilot"
            description="Side panel chat and proposals"
            checked={settings.modules_enabled.copilot}
            disabled={!canManage}
            onCheckedChange={(v) =>
              setSettings({
                ...settings,
                modules_enabled: { ...settings.modules_enabled, copilot: v },
              })
            }
          />
          <Switch
            label="Insights"
            description="Dashboard observations"
            checked={settings.modules_enabled.insights}
            disabled={!canManage}
            onCheckedChange={(v) =>
              setSettings({
                ...settings,
                modules_enabled: { ...settings.modules_enabled, insights: v },
              })
            }
          />
          <Switch
            label="Document AI"
            description="Receipt and invoice extraction"
            checked={settings.modules_enabled.document_ai}
            disabled={!canManage}
            onCheckedChange={(v) =>
              setSettings({
                ...settings,
                modules_enabled: { ...settings.modules_enabled, document_ai: v },
              })
            }
          />
          <Switch
            label="Ask mode"
            description="Command bar natural-language queries"
            checked={settings.modules_enabled.ask_mode}
            disabled={!canManage}
            onCheckedChange={(v) =>
              setSettings({
                ...settings,
                modules_enabled: { ...settings.modules_enabled, ask_mode: v },
              })
            }
          />
        </fieldset>

        <Select
          label="Model preference"
          value={settings.model_preference}
          disabled={!canManage}
          onChange={(e) => setSettings({ ...settings, model_preference: e.target.value })}
          options={MODEL_OPTIONS}
        />

        <label style={label}>
          Auto-execute allow list
          <Input
            value={allowListText}
            onChange={(e) => setAllowListText(e.target.value)}
            disabled={!canManage}
            placeholder="create_task, draft_follow_up_activity"
          />
          <span style={hint}>
            Comma-separated action types that may run without confirmation. Leave empty for safest default.
          </span>
        </label>
        {allowListText.trim() ? (
          <InlineAlert tone="warning" title="Auto-execute enabled">
            Actions in the allow list can commit without an explicit confirm step.
          </InlineAlert>
        ) : (
          <InlineAlert tone="info" title="Confirmation required">
            All write proposals require user confirmation.
          </InlineAlert>
        )}

        <fieldset style={fieldset}>
          <legend style={legend}>Data sharing</legend>
          <Checkbox
            label="Share with model provider"
            checked={settings.data_sharing.share_with_provider}
            disabled={!canManage}
            onChange={(e) =>
              setSettings({
                ...settings,
                data_sharing: {
                  ...settings.data_sharing,
                  share_with_provider: e.target.checked,
                },
              })
            }
          />
          <Checkbox
            label="Allow training on org data"
            checked={settings.data_sharing.allow_training}
            disabled={!canManage}
            onChange={(e) =>
              setSettings({
                ...settings,
                data_sharing: {
                  ...settings.data_sharing,
                  allow_training: e.target.checked,
                },
              })
            }
          />
        </fieldset>

        <label style={label}>
          Monthly token budget
          <Input
            type="number"
            value={String(settings.monthly_token_budget)}
            onChange={(e) =>
              setSettings({
                ...settings,
                monthly_token_budget: Number(e.target.value) || 0,
              })
            }
            disabled={!canManage}
            min={0}
          />
        </label>

        <div style={usageBox}>
          <p style={{ margin: 0, fontSize: '0.9rem' }}>
            Tokens used this month ({settings.budget_month}):{' '}
            <strong>{settings.tokens_used_this_month.toLocaleString()}</strong>
            {settings.monthly_token_budget > 0 ? (
              <> / {settings.monthly_token_budget.toLocaleString()} ({budgetPct}%)</>
            ) : null}
          </p>
        </div>

        {canManage ? (
          <Button type="submit" disabled={busy}>
            {busy ? 'Saving…' : 'Save AI settings'}
          </Button>
        ) : (
          <EmptyState title="View only" description="You need ai.settings.manage to edit these fields." />
        )}
      </form>
    </div>
  );
}

const eyebrow: CSSProperties = {
  margin: 0,
  textTransform: 'uppercase',
  letterSpacing: '0.08em',
  fontSize: '0.72rem',
  color: 'var(--cos-color-fg-muted)',
  fontWeight: 600,
};

const h1: CSSProperties = {
  margin: '0.35rem 0 0',
  fontFamily: 'var(--cos-font-display)',
  fontSize: '1.75rem',
  fontWeight: 650,
};

const muted: CSSProperties = {
  margin: '0.4rem 0 0',
  color: 'var(--cos-color-fg-muted)',
};

const fieldset: CSSProperties = {
  border: '1px solid var(--cos-color-border)',
  borderRadius: 'var(--cos-radius-sm)',
  padding: '0.75rem 1rem',
  display: 'grid',
  gap: 12,
  margin: 0,
};

const legend: CSSProperties = {
  padding: '0 0.35rem',
  fontSize: '0.85rem',
  fontWeight: 600,
  color: 'var(--cos-color-fg)',
};

const label: CSSProperties = {
  display: 'grid',
  gap: '0.35rem',
  fontSize: '0.85rem',
  color: 'var(--cos-color-fg-muted)',
};

const hint: CSSProperties = {
  fontSize: '0.78rem',
  color: 'var(--cos-color-fg-muted)',
};

const usageBox: CSSProperties = {
  padding: '0.65rem 0.75rem',
  borderRadius: 'var(--cos-radius-sm)',
  border: '1px solid var(--cos-color-border)',
  background: 'var(--cos-color-bg-muted)',
};
