export function ComingSoon({ screen }: { screen: string }) {
  return (
    <>
      <header className="page-header">
        <h1>{screen}</h1>
      </header>
      <section className="card empty">
        <p>This screen is not part of the prototype yet.</p>
      </section>
    </>
  );
}
