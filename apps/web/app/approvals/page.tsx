import { EmptyState } from '@companyos/design-system';

export default function ApprovalsPage() {
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
        Approvals
      </h1>
      <div style={{ marginTop: '1rem' }}>
        <EmptyState
          title="Nothing to approve"
          description="Approval queues arrive with finance and ops workflows in later phases."
        />
      </div>
    </section>
  );
}
