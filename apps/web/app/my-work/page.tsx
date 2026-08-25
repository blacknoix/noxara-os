import { EmptyState } from '@companyos/design-system';

export default function MyWorkPage() {
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
        My work
      </h1>
      <div style={{ marginTop: '1rem' }}>
        <EmptyState
          title="No work items yet"
          description="Tasks and follow-ups assigned to you will show up here."
        />
      </div>
    </section>
  );
}
