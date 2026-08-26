'use client';

import { useState, type CSSProperties, type FormEvent } from 'react';
import Link from 'next/link';
import {
  Badge,
  Button,
  EmptyState,
  ErrorState,
  MoneyCell,
  Select,
  Textarea,
} from '@companyos/design-system';
import { confirmProposal, extractDocument } from '../../../lib/ai-api';
import type { DocumentReview } from '../../../lib/ai-types';
import { getAccessToken } from '../../../lib/auth-client';

const KIND_OPTIONS = [
  { value: 'expense', label: 'Expense / receipt' },
  { value: 'invoice', label: 'Invoice' },
];

export default function DocumentAiPage() {
  const [kind, setKind] = useState<'expense' | 'invoice'>('expense');
  const [text, setText] = useState('');
  const [fileId, setFileId] = useState('');
  const [review, setReview] = useState<DocumentReview | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (!getAccessToken()) {
    return <ErrorState title="Sign in required" message="Open /login to use Document AI." />;
  }

  async function onExtract(e: FormEvent) {
    e.preventDefault();
    setError(null);
    setReview(null);
    if (!text.trim() && !fileId.trim()) {
      setError('Paste document text or provide a file ID.');
      return;
    }
    setBusy(true);
    try {
      const result = await extractDocument({
        kind,
        text: text.trim() || undefined,
        file_id: fileId.trim() || undefined,
      });
      if (!result) {
        setError('Extraction failed. Check permissions or try again.');
        return;
      }
      setReview(result);
    } finally {
      setBusy(false);
    }
  }

  async function onConfirm() {
    if (!review?.proposal_id) return;
    setBusy(true);
    try {
      const confirmed = await confirmProposal(review.proposal_id);
      if (!confirmed) {
        setError('Could not confirm proposal.');
        return;
      }
      setReview({ ...review, status: confirmed.status });
    } finally {
      setBusy(false);
    }
  }

  const amountMinor =
    typeof review?.extracted.amount_minor === 'number' ? review.extracted.amount_minor : null;
  const currency =
    typeof review?.extracted.currency === 'string' ? review.extracted.currency : 'USD';

  return (
    <section>
      <header style={headerStyle}>
        <div>
          <p style={eyebrow}>Finance · Document AI</p>
          <h1 style={title}>Extract from document</h1>
          <p style={muted}>
            Paste receipt or invoice text for structured extraction.{' '}
            <Link href="/finance/expenses" style={{ color: 'var(--cos-color-accent)' }}>
              Back to expenses
            </Link>
          </p>
        </div>
      </header>

      <form onSubmit={(e) => void onExtract(e)} style={formStyle}>
        <Select
          label="Document kind"
          value={kind}
          onChange={(e) => setKind(e.target.value as 'expense' | 'invoice')}
          options={KIND_OPTIONS}
        />
        <Textarea
          label="Document text"
          hint="Paste OCR or plain text from a receipt or invoice"
          value={text}
          onChange={(e) => setText(e.target.value)}
          rows={8}
        />
        <label style={labelStyle}>
          File ID (optional)
          <input
            value={fileId}
            onChange={(e) => setFileId(e.target.value)}
            placeholder="file_…"
            style={inputStyle}
          />
        </label>
        {error ? <ErrorState message={error} /> : null}
        <Button type="submit" disabled={busy}>
          {busy ? 'Extracting…' : 'Extract fields'}
        </Button>
      </form>

      {review ? (
        <div style={reviewStyle}>
          <div style={{ display: 'flex', gap: 8, alignItems: 'center', flexWrap: 'wrap' }}>
            <h2 style={{ margin: 0, fontSize: '1.1rem' }}>Extraction result</h2>
            <Badge tone="neutral">{review.kind}</Badge>
            <Badge tone={review.confidence >= 0.8 ? 'success' : 'warning'}>
              {Math.round(review.confidence * 100)}% confidence
            </Badge>
            <Badge>{review.status}</Badge>
          </div>

          <dl style={dlStyle}>
            {amountMinor !== null ? (
              <>
                <dt>Amount</dt>
                <dd>
                  <MoneyCell amount={amountMinor / 100} currency={currency} />
                </dd>
              </>
            ) : null}
            {typeof review.extracted.vendor === 'string' ? (
              <>
                <dt>Vendor</dt>
                <dd>{review.extracted.vendor}</dd>
              </>
            ) : null}
            {typeof review.extracted.description === 'string' ? (
              <>
                <dt>Description</dt>
                <dd>{review.extracted.description}</dd>
              </>
            ) : null}
            {typeof review.extracted.date === 'string' ? (
              <>
                <dt>Date</dt>
                <dd>{review.extracted.date}</dd>
              </>
            ) : null}
          </dl>

          {review.proposal_id && review.status === 'pending' ? (
            <Button type="button" onClick={() => void onConfirm()} disabled={busy}>
              Confirm proposal
            </Button>
          ) : review.status === 'committed' ? (
            <EmptyState title="Committed" description="The extracted record was created." />
          ) : null}
        </div>
      ) : null}
    </section>
  );
}

const headerStyle: CSSProperties = { marginBottom: '1.25rem' };
const eyebrow: CSSProperties = {
  margin: 0,
  fontSize: '0.75rem',
  letterSpacing: '0.06em',
  textTransform: 'uppercase',
  color: 'var(--cos-color-fg-muted)',
};
const title: CSSProperties = { margin: '0.25rem 0', fontSize: '1.75rem', fontFamily: 'var(--cos-font-display)' };
const muted: CSSProperties = { margin: '0.35rem 0 0', color: 'var(--cos-color-fg-muted)' };
const formStyle: CSSProperties = { display: 'grid', gap: 12, maxWidth: 560, marginBottom: '1.5rem' };
const labelStyle: CSSProperties = {
  display: 'grid',
  gap: 4,
  fontSize: '0.85rem',
  color: 'var(--cos-color-fg-muted)',
};
const inputStyle: CSSProperties = {
  border: '1px solid var(--cos-color-border)',
  borderRadius: 'var(--cos-radius-sm)',
  padding: '0.45rem 0.6rem',
  background: 'var(--cos-color-bg-elevated)',
  color: 'var(--cos-color-fg)',
};
const reviewStyle: CSSProperties = {
  padding: '1rem 1.1rem',
  border: '1px solid var(--cos-color-border)',
  borderRadius: 'var(--cos-radius-md)',
  background: 'var(--cos-color-bg-elevated)',
  display: 'grid',
  gap: 12,
  maxWidth: 560,
};
const dlStyle: CSSProperties = {
  display: 'grid',
  gridTemplateColumns: '120px 1fr',
  gap: '0.35rem 1rem',
  margin: 0,
  fontSize: '0.9rem',
};
