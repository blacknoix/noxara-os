export type CosTheme = 'light' | 'dark' | 'high-contrast';

export const THEME_STORAGE_KEY = 'cos-theme';

export const THEME_OPTIONS: { value: CosTheme; label: string }[] = [
  { value: 'light', label: 'Light' },
  { value: 'dark', label: 'Dark' },
  { value: 'high-contrast', label: 'High contrast' },
];

export function isCosTheme(value: string | null | undefined): value is CosTheme {
  return value === 'light' || value === 'dark' || value === 'high-contrast';
}

/** Resolve initial theme from localStorage, else prefers-color-scheme. */
export function resolveInitialTheme(): CosTheme {
  if (typeof window === 'undefined') return 'light';
  try {
    const stored = window.localStorage.getItem(THEME_STORAGE_KEY);
    if (isCosTheme(stored)) return stored;
  } catch {
    /* ignore */
  }
  if (window.matchMedia?.('(prefers-color-scheme: dark)').matches) {
    return 'dark';
  }
  return 'light';
}

export function applyTheme(theme: CosTheme) {
  if (typeof document === 'undefined') return;
  document.documentElement.setAttribute('data-theme', theme);
  try {
    window.localStorage.setItem(THEME_STORAGE_KEY, theme);
  } catch {
    /* ignore */
  }
}
