import { EmptyState } from '@companyos/design-system';

export default function PlaceholderPage({
  title,
  description = 'This module is not enabled yet. No placeholder records.',
}: {
  title: string;
  description?: string;
}) {
  return (
    <section>
      <h1
        style={{
          margin: 0,
          fontFamily: 'var(--cos-font-display)',
          fontSize: '1.75rem',
          fontWeight: 650,
        }}
      >
        {title}
      </h1>
      <div style={{ marginTop: '1rem' }}>
        <EmptyState title="Coming later" description={description} />
      </div>
    </section>
  );
}
