'use client';

import { useCallback, useEffect, useState } from 'react';
import { authFetch, getAccessToken } from './auth-client';

export type Capabilities = {
  org_id: string;
  role: string;
  policy_version: number;
  allowed: string[];
};

let cached: Capabilities | null = null;

export function useCapabilities() {
  const [caps, setCaps] = useState<Capabilities | null>(cached);
  const [loading, setLoading] = useState(!cached);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!getAccessToken()) {
      setCaps(null);
      setLoading(false);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const res = await authFetch('/api/v1/workspace/me/capabilities');
      if (res.status === 401) {
        setCaps(null);
        setError('Sign in required');
        return;
      }
      if (res.status === 403) {
        setError('Permission denied');
        setCaps(null);
        return;
      }
      if (!res.ok) {
        setError('Could not load capabilities');
        return;
      }
      const body = (await res.json()) as Capabilities;
      cached = body;
      setCaps(body);
    } catch {
      setError('Capabilities request failed');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    const onOrgSwitch = () => {
      cached = null;
      void refresh();
    };
    window.addEventListener('cos:org-switched', onOrgSwitch);
    return () => window.removeEventListener('cos:org-switched', onOrgSwitch);
  }, [refresh]);

  const can = (permission: string) => caps?.allowed.includes(permission) ?? false;

  return { caps, loading, error, can, refresh };
}

export function clearCapabilitiesCache() {
  cached = null;
}
