import { EmptyState } from '@companyos/design-system';

export default function InboxPage() {
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
        Inbox
      </h1>
      <div style={{ marginTop: '1rem' }}>
        <EmptyState
          title="Inbox is empty"
          description="Notifications and assignments will appear here in a later phase."
        />
      </div>
    </section>
  );
}
