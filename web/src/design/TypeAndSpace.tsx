const roles = [
  ["display", "Display · 28 / 1.15", "A place for your work"],
  ["heading", "Heading · 20 / 1.25", "Clear signals, considered details"],
  ["body", "Body · 15 / 1.6", "Keep the work in view, with room to think."],
  ["body-strong", "Body strong · 15 / 1.6", "Workspace name"],
  ["small", "Small · 13 / 1.5", "Updated a moment ago"],
  ["label", "Label · 12 / 1.6", "Run status"],
  ["code", "Code · 13 / 1.55", 'run.status = "idle";'],
] as const;
const spaces = [1, 2, 3, 4, 6, 8, 12, 16];

/** Shows every approved typography role, spacing step and radius. */
export function TypeAndSpace() {
  return (
    <>
      <section className="design-section">
        <h3 className="type-heading">Typography</h3>
        {roles.map(([role, label, sample]) => (
          <div className="type-sample" key={role}>
            <p className="type-label muted">{label}</p>
            <p className={`type-${role}`}>{sample}</p>
          </div>
        ))}
      </section>
      <section className="design-section">
        <h3 className="type-heading">Spacing and radii</h3>
        <div className="specimen-grid">
          {spaces.map((space) => (
            <div className="spacing-specimen" key={space}>
              <code className="type-label">{space * 4} px</code>
              <span
                className="spacing-bar"
                style={{ width: `var(--space-${space})` }}
              />
            </div>
          ))}
        </div>
        <div className="row">
          {["control", "dialog", "pill"].map((radius) => (
            <span
              key={radius}
              className="radius-specimen type-label"
              style={{ borderRadius: `var(--${radius}-radius)` }}
            >
              {radius}
            </span>
          ))}
        </div>
      </section>
    </>
  );
}
