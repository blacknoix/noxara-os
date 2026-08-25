import type { CSSProperties, HTMLAttributes } from 'react';

export type SkeletonProps = HTMLAttributes<HTMLDivElement> & {
  width?: number | string;
  height?: number | string;
  circle?: boolean;
};

export function Skeleton({ width = '100%', height = 16, circle, style, ...rest }: SkeletonProps) {
  const base: CSSProperties = {
    width: circle ? height : width,
    height,
    borderRadius: circle ? '50%' : 'var(--cos-radius-sm)',
    background: 'linear-gradient(90deg, var(--cos-color-bg-muted) 25%, var(--cos-color-border) 50%, var(--cos-color-bg-muted) 75%)',
    backgroundSize: '200% 100%',
    animation: `cos-skeleton var(--cos-duration-slow) ease-in-out infinite`,
    ...style,
  };
  return (
    <>
      <style>{`@keyframes cos-skeleton { 0% { background-position: 200% 0; } 100% { background-position: -200% 0; } }`}</style>
      <div {...rest} aria-hidden="true" style={base} />
    </>
  );
}
