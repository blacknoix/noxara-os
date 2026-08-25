import type { HTMLAttributes, ReactNode } from 'react';

export type CardProps = HTMLAttributes<HTMLDivElement> & {
  children: ReactNode;
  as?: 'div' | 'section' | 'article';
};

/** Interaction/container surface — use when a bordered region aids understanding. */
export function Card({ children, as: Comp = 'div', style, ...rest }: CardProps) {
  return (
    <Comp
      {...rest}
      style={{
        background: 'var(--cos-color-bg-elevated)',
        border: '1px solid var(--cos-color-border)',
        borderRadius: 'var(--cos-radius-md)',
        padding: 'var(--cos-space-4)',
        fontFamily: 'var(--cos-font-sans)',
        boxShadow: 'var(--cos-shadow-soft)',
        ...style,
      }}
    >
      {children}
    </Comp>
  );
}
