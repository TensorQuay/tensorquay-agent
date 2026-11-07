import type { TokenValue } from "./use-token-values";

/** Labels each colour swatch with its resolved value and measured contrast. */
export function TokenPalette({ values }: { values: readonly TokenValue[] }) {
  return (
    <section className="design-section">
      <h3 className="type-heading">Colour tokens</h3>
      <div className="token-list">
        {values.map((token) => (
          <div className="token-item" key={token.name}>
            <span
              className="token-chip"
              aria-hidden="true"
              style={{ backgroundColor: `var(--${token.name})` }}
            />
            <div>
              <code className="type-label">--{token.name}</code>
              <p className="type-small muted">{token.value}</p>
              <p className="type-small muted">
                {token.ratio.toFixed(2)}:1 on {token.against}
              </p>
            </div>
          </div>
        ))}
      </div>
      <p className="type-small muted">
        Text ≥ 4.5:1. Accent and focus ≥ 3:1. Line is a decorative divider.
      </p>
    </section>
  );
}
