import type { CSSProperties, HTMLAttributes } from 'react';

export type AvatarSize = 'sm' | 'md' | 'lg';

export type AvatarProps = HTMLAttributes<HTMLSpanElement> & {
  name: string;
  src?: string;
  size?: AvatarSize;
};

const sizes: Record<AvatarSize, number> = { sm: 28, md: 36, lg: 48 };

function initials(name: string) {
  const parts = name.trim().split(/\s+/).slice(0, 2);
  return parts.map((p) => p[0]?.toUpperCase() ?? '').join('') || '?';
}

export function Avatar({ name, src, size = 'md', style, ...rest }: AvatarProps) {
  const dim = sizes[size];
  const base: CSSProperties = {
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    width: dim,
    height: dim,
    borderRadius: '50%',
    background: 'var(--cos-color-accent-muted)',
    color: 'var(--cos-color-accent)',
    fontFamily: 'var(--cos-font-sans)',
    fontWeight: 600,
    fontSize: dim * 0.35,
    overflow: 'hidden',
    flexShrink: 0,
    ...style,
  };

  if (src) {
    return (
      <span {...rest} role="img" aria-label={name} style={base}>
        <img src={src} alt="" width={dim} height={dim} style={{ width: '100%', height: '100%', objectFit: 'cover' }} />
      </span>
    );
  }

  return (
    <span {...rest} role="img" aria-label={name} style={base}>
      {initials(name)}
    </span>
  );
}
