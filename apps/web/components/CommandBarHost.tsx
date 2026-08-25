'use client';

import { useEffect, useMemo, useState } from 'react';
import { useRouter } from 'next/navigation';
import { CommandBar, type CommandItem, useToast } from '@companyos/design-system';
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
      return;
    }
    if (!getAccessToken()) return;

    void (async () => {
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

  const items: CommandItem[] = useMemo(() => {
    const list: CommandItem[] = [];
    const q = query.trim().toLowerCase();

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

    const askLabel = 'Ask AI (comes in phase 1.9)';
    if (!q || askLabel.toLowerCase().includes(q) || 'ask'.includes(q)) {
      list.push({
        id: 'ask-ai',
        label: askLabel,
        group: 'Ask',
        onSelect: () => {
          toast({
            title: 'AI arrives in phase 1.9',
            description: 'Ask and copilot features are not available yet.',
          });
        },
      });
    }

    return list;
  }, [members, memberships, query, can, router, theme, setTheme, onToggleSidebar, toast]);

  return (
    <CommandBar
      open={open}
      onOpenChange={onOpenChange}
      query={query}
      onQueryChange={setQuery}
      items={items}
      placeholder="Search members, run a command, or ask…"
      emptyMessage="No matching results"
    />
  );
}
