'use client';

import { useCallback, useEffect, useState } from 'react';
import { Badge, Button } from '@companyos/design-system';
import { cancelProposal, confirmProposal, fetchSuggestions } from '../lib/ai-api';
import type { SuggestionChip } from '../lib/ai-types';
import { getAccessToken } from '../lib/auth-client';

export function AiSuggestionChips({
  pageScope,
  recordId,
}: {
  pageScope?: string;
  recordId?: string;
}) {
  const [chips, setChips] = useState<SuggestionChip[]>([]);
  const [busyId, setBusyId] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!getAccessToken()) {
      setChips([]);
      return;
    }
    const items = await fetchSuggestions(pageScope, recordId);
    setChips(items);
  }, [pageScope, recordId]);

  useEffect(() => {
    void load();
  }, [load]);

  if (chips.length === 0) return null;

  async function onConfirm(chip: SuggestionChip) {
    if (!chip.proposal_id) return;
    setBusyId(chip.id);
    try {
      await confirmProposal(chip.proposal_id);
      setChips((prev) => prev.filter((c) => c.id !== chip.id));
    } finally {
      setBusyId(null);
    }
  }

  async function onCancel(chip: SuggestionChip) {
    if (!chip.proposal_id) return;
    setBusyId(chip.id);
    try {
      await cancelProposal(chip.proposal_id);
      setChips((prev) => prev.filter((c) => c.id !== chip.id));
    } finally {
      setBusyId(null);
    }
  }

  return (
    <div
      role="group"
      aria-label="AI suggestions"
      style={{
        display: 'flex',
        flexWrap: 'wrap',
        gap: 8,
        marginBottom: '1rem',
        alignItems: 'center',
      }}
    >
      <span style={{ fontSize: '0.8rem', color: 'var(--cos-color-fg-muted)' }}>Suggestions</span>
      {chips.map((chip) => (
        <div
          key={chip.id}
          style={{
            display: 'inline-flex',
            alignItems: 'center',
            gap: 6,
            padding: '0.3rem 0.5rem',
            border: '1px solid var(--cos-color-border)',
            borderRadius: 'var(--cos-radius-sm)',
            background: 'var(--cos-color-bg-elevated)',
            fontSize: '0.82rem',
          }}
        >
          <Badge tone="neutral">{chip.action_type}</Badge>
          <span>{chip.label}</span>
          <Button
            type="button"
            size="sm"
            disabled={busyId === chip.id}
            onClick={() => void onConfirm(chip)}
          >
            Confirm
          </Button>
          <Button
            type="button"
            size="sm"
            variant="secondary"
            disabled={busyId === chip.id}
            onClick={() => void onCancel(chip)}
          >
            Cancel
          </Button>
        </div>
      ))}
    </div>
  );
}
