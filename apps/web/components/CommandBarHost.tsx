'use client';

import { useEffect, useMemo, useState, type FormEvent } from 'react';
import { useRouter } from 'next/navigation';
import {
  Button,
  CommandBar,
  Input,
  Modal,
  Textarea,
  type CommandItem,
  useToast,
} from '@companyos/design-system';
import { askAi, confirmProposal, sendChat } from '../lib/ai-api';
import type { AskForm, AskFormField, Citation } from '../lib/ai-types';
import { isUuid } from '../lib/ai-types';
import { authFetch, getAccessToken } from '../lib/auth-client';
import { useCapabilities } from '../lib/capabilities';
import { type CosTheme } from '../lib/theme';
import { useTheme } from './ThemeProvider';

type Member = {
  membership_id: string;
  email: string;
  display_name: string;
};

type Membership = { org_id: string; org_name: string; role: string };

type SearchHit = {
  doc_id: string;
  doc_type: string;
  title: string;
  body: string;
  href?: string | null;
};

function hrefForHit(hit: SearchHit): string {
  if (hit.href) return hit.href;
  const q = encodeURIComponent(hit.title);
  switch (hit.doc_type) {
    case 'customer':
    case 'deal':
    case 'lead':
    case 'quote':
      return `/sales?q=${q}`;
    case 'invoice':
    case 'expense':
      return `/finance/invoices`;
    case 'task':
      return `/ops/tasks`;
    case 'project':
      return `/ops/projects`;
    default:
      return `/sales?q=${q}`;
  }
}

export function CommandBarHost({
  open,
  onOpenChange,
  onToggleSidebar,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onToggleSidebar: () => void;
}) {
  const router = useRouter();
  const { toast } = useToast();
  const { theme, setTheme } = useTheme();
  const { can, caps } = useCapabilities();
  const [query, setQuery] = useState('');
  const [members, setMembers] = useState<Member[]>([]);
  const [memberships, setMemberships] = useState<Membership[]>([]);
  const [orgId, setOrgId] = useState<string | null>(null);
  const [searchHits, setSearchHits] = useState<SearchHit[]>([]);
  const [askForm, setAskForm] = useState<AskForm | null>(null);
  const [askQuery, setAskQuery] = useState('');
  const [formFields, setFormFields] = useState<AskFormField[]>([]);
  const [askBusy, setAskBusy] = useState(false);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        onOpenChange(!open);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [open, onOpenChange]);

  useEffect(() => {
    if (!open) {
      setQuery('');
      setSearchHits([]);
      return;
    }
    if (!getAccessToken()) return;

    void (async () => {
      const meRes = await authFetch('/api/v1/auth/me');
      if (meRes.ok) {
        const me = await meRes.json();
        if (me.org_id) setOrgId(me.org_id);
      }
      const memRes = await authFetch('/api/v1/auth/memberships');
      if (memRes.ok) {
        const body = await memRes.json();
        setMemberships(body.items ?? []);
      }
      if (caps?.allowed.includes('workspace.member.read')) {
        const res = await authFetch('/api/v1/workspace/members');
        if (res.ok) {
          const body = await res.json();
          setMembers(body.items ?? []);
        }
      } else {
        setMembers([]);
      }
    })();
  }, [open, caps]);

  useEffect(() => {
    if (!open || !orgId || query.trim().length < 2) {
      setSearchHits([]);
      return;
    }
    if (!getAccessToken()) return;
    const q = query.trim();
    const handle = window.setTimeout(() => {
      void (async () => {
        const res = await authFetch(
          `/api/v1/search/query?q=${encodeURIComponent(q)}&org_id=${encodeURIComponent(orgId)}`,
        );
        if (!res.ok) {
          setSearchHits([]);
          return;
        }
        const body = await res.json();
        setSearchHits(body.hits ?? []);
      })();
    }, 200);
    return () => window.clearTimeout(handle);
  }, [open, orgId, query]);

  const items: CommandItem[] = useMemo(() => {
    const list: CommandItem[] = [];
    const q = query.trim().toLowerCase();

    for (const hit of searchHits) {
      list.push({
        id: `search-${hit.doc_type}-${hit.doc_id}`,
        label: `${hit.title} · ${hit.doc_type}`,
        group: 'Search',
        onSelect: () => router.push(hrefForHit(hit)),
      });
    }

    for (const m of members) {
      const label = `${m.display_name} · ${m.email}`;
      if (q && !label.toLowerCase().includes(q)) continue;
      list.push({
        id: `member-${m.membership_id}`,
        label,
        group: 'Search',
        onSelect: () => router.push(`/members?q=${encodeURIComponent(m.email)}`),
      });
    }

    for (const org of memberships) {
      const label = `Organization · ${org.org_name}`;
      if (q && !label.toLowerCase().includes(q) && !org.org_name.toLowerCase().includes(q)) continue;
      list.push({
        id: `org-${org.org_id}`,
        label,
        group: 'Search',
        onSelect: () => {
          toast({
            title: 'Switch organization',
            description: `Use the org switcher in the top bar to open ${org.org_name}.`,
          });
        },
      });
    }

    const commands: { id: string; label: string; show: boolean; onSelect: () => void; shortcut?: string }[] = [
      {
        id: 'cmd-dashboard',
        label: 'Go to dashboard',
        show: true,
        onSelect: () => router.push('/'),
      },
      {
        id: 'cmd-create-org',
        label: 'Create organization',
        show: true,
        onSelect: () => router.push('/onboarding'),
      },
      {
        id: 'cmd-invite',
        label: 'Invite member',
        show: can('workspace.member.invite'),
        onSelect: () => router.push('/members'),
      },
      {
        id: 'cmd-settings',
        label: 'Open settings',
        show: can('workspace.org.read'),
        onSelect: () => router.push('/settings'),
      },
      {
        id: 'cmd-theme-light',
        label: 'Switch theme to light',
        show: theme !== 'light',
        onSelect: () => setTheme('light' as CosTheme),
      },
      {
        id: 'cmd-theme-dark',
        label: 'Switch theme to dark',
        show: theme !== 'dark',
        onSelect: () => setTheme('dark' as CosTheme),
      },
      {
        id: 'cmd-theme-hc',
        label: 'Switch theme to high contrast',
        show: theme !== 'high-contrast',
        onSelect: () => setTheme('high-contrast' as CosTheme),
      },
      {
        id: 'cmd-sidebar',
        label: 'Toggle sidebar',
        show: true,
        onSelect: () => onToggleSidebar(),
      },
    ];

    for (const c of commands) {
      if (!c.show) continue;
      if (q && !c.label.toLowerCase().includes(q)) continue;
      list.push({
        id: c.id,
        label: c.label,
        group: 'Commands',
        shortcut: c.shortcut,
        onSelect: c.onSelect,
      });
    }

    const askText = query.trim().replace(/^ask\s+/i, '').trim() || query.trim();
    const looksLikeQuestion =
      askText.length > 2 &&
      (/^(ask\s+)/i.test(query.trim()) ||
        askText.endsWith('?') ||
        /^(what|who|when|where|why|how|which|show|list|find)\b/i.test(askText));

    if (looksLikeQuestion && getAccessToken()) {
      list.push({
        id: 'ask-ai',
        label: `Ask: ${askText}`,
        group: 'Ask',
        onSelect: () => void runAsk(askText),
      });
    }

    return list;
  }, [members, memberships, searchHits, query, can, router, theme, setTheme, onToggleSidebar, toast]);

  async function runAsk(askText: string) {
    onOpenChange(false);
    setAskQuery(askText);
    const response = await askAi(askText);
    if (!response) {
      toast({ title: 'Ask failed', description: 'Could not reach AI. Sign in or try again.' });
      return;
    }

    if (response.kind === 'form' && response.form) {
      setFormFields(response.form.fields.map((f) => ({ ...f })));
      setAskForm(response.form);
      return;
    }

    if (response.kind === 'read') {
      const citations = response.citations ?? [];
      toast({
        title: response.message ?? 'Answer',
        description: formatCitations(citations),
      });
    }
  }

  function formatCitations(citations: Citation[]): string {
    if (citations.length === 0) return 'No matching records found.';
    return citations.map((c) => c.title).join(' · ');
  }

  async function submitAskForm(e: FormEvent) {
    e.preventDefault();
    if (!askForm) return;
    setAskBusy(true);
    try {
      const preview = askForm.proposal_preview;
      if (preview && isUuid(preview)) {
        const result = await confirmProposal(preview);
        if (result) {
          toast({ title: 'Proposal confirmed', description: result.rendered_diff });
          setAskForm(null);
          return;
        }
      }

      const summary = formFields.map((f) => `${f.label}: ${f.value}`).join(', ');
      const message = `${askForm.action_type} — ${summary}`;
      const chat = await sendChat({ message });
      if (chat?.proposals.length) {
        const proposal = chat.proposals[0]!;
        const confirmed = await confirmProposal(proposal.id);
        toast({
          title: confirmed ? 'Created' : 'Proposal ready',
          description: confirmed?.rendered_diff ?? proposal.rendered_diff,
        });
      } else {
        toast({ title: 'Submitted', description: chat?.content ?? message });
      }
      setAskForm(null);
    } finally {
      setAskBusy(false);
    }
  }

  return (
    <>
      <CommandBar
        open={open}
        onOpenChange={onOpenChange}
        query={query}
        onQueryChange={setQuery}
        items={items}
        placeholder="Search members, run a command, or ask…"
        emptyMessage="No matching results"
      />
      <Modal
        open={askForm !== null}
        onClose={() => setAskForm(null)}
        title={`Ask: ${askQuery}`}
        footer={
          <>
            <Button type="button" variant="secondary" onClick={() => setAskForm(null)}>
              Cancel
            </Button>
            <Button type="submit" form="ask-form" disabled={askBusy}>
              {askBusy ? 'Submitting…' : 'Submit'}
            </Button>
          </>
        }
      >
        <form id="ask-form" onSubmit={(e) => void submitAskForm(e)} style={{ display: 'grid', gap: 12 }}>
          {formFields.map((field, idx) => (
            <label
              key={field.name}
              style={{ display: 'grid', gap: 4, fontSize: '0.85rem', color: 'var(--cos-color-fg-muted)' }}
            >
              {field.label}
              {field.type === 'textarea' ? (
                <Textarea
                  value={field.value}
                  onChange={(e) => {
                    const next = [...formFields];
                    next[idx] = { ...field, value: e.target.value };
                    setFormFields(next);
                  }}
                />
              ) : (
                <Input
                  type={field.type === 'number' ? 'number' : 'text'}
                  value={field.value}
                  onChange={(e) => {
                    const next = [...formFields];
                    next[idx] = { ...field, value: e.target.value };
                    setFormFields(next);
                  }}
                />
              )}
            </label>
          ))}
        </form>
      </Modal>
    </>
  );
}
