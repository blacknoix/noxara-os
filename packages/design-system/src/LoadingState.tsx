import { Skeleton } from './Skeleton';

export type LoadingStateProps = {
  label?: string;
  rows?: number;
};

export function LoadingState({ label = 'Loading', rows = 4 }: LoadingStateProps) {
  return (
    <div
      role="status"
      aria-live="polite"
      aria-busy="true"
      style={{
        padding: 'var(--cos-space-6) var(--cos-space-4)',
        fontFamily: 'var(--cos-font-sans)',
      }}
    >
      <span
        style={{
          position: 'absolute',
          width: 1,
          height: 1,
          overflow: 'hidden',
          clip: 'rect(0,0,0,0)',
        }}
      >
        {label}
      </span>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 10, maxWidth: 480, margin: '0 auto' }}>
        {Array.from({ length: rows }).map((_, i) => (
          <Skeleton key={i} height={14} width={`${90 - i * 8}%`} />
        ))}
      </div>
    </div>
  );
}
