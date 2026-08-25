export default function PlaceholderPage({
  title,
}: {
  title: string;
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
      <p style={{ color: 'var(--cos-color-fg-muted)' }}>Phase 0 placeholder — no product data yet.</p>
    </section>
  );
}
