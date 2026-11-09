import Link from "next/link";

/**
 * A table cell's one muted line (UI-GUIDELINES §7 Table, 17.3): cut with "…" where its column ends, its words whole in
 * its tooltip; the item's own page says them in full. A line that shows its figures short takes them `exact` too (§10,
 * WEB-U-461): its words without a link, or the words `after` its link. The short words show; the tooltip and screen
 * readers get the exact ones, as `ShortAndExact` does. With `href`, the line holds a link of its words, never cut: after
 * the reason `before` it (where to allow a host), or ahead of the words `after` it (where to add a missing key). A
 * space comes first, so the row's text and its name read "value note", never "valuenote".
 */
export function CellNote({
  children,
  href,
  before,
  after,
  exact,
}: {
  children: string;
  href?: string;
  before?: string | null;
  after?: string | null;
  exact?: string | null;
}) {
  const line = (main: string, tail?: string | null) =>
    [before, [main, tail].filter(Boolean).join(" · ")]
      .filter(Boolean)
      .join(" ");
  const words = line(children, after);
  const whole = exact
    ? href
      ? line(children, exact)
      : line(exact, after)
    : words;
  return (
    <>
      {" "}
      <span className="type-small muted cell-note" title={whole}>
        {href ? (
          <>
            {before ? <span>{`${before} `}</span> : null}
            <Link href={href} className="cell-note-link">
              {children}
            </Link>
            {after ? (
              <Shown
                short={` · ${after}`}
                exact={exact ? ` · ${exact}` : null}
              />
            ) : null}
          </>
        ) : exact ? (
          <Shown short={words} exact={whole} />
        ) : (
          words
        )}
      </span>
    </>
  );
}

/** Words shown short and hidden from screen readers, beside the exact words only screen readers get; plain without. */
function Shown({ short, exact }: { short: string; exact?: string | null }) {
  return exact ? (
    <>
      <span aria-hidden="true">{short}</span>
      <span className="visually-hidden">{exact}</span>
    </>
  ) : (
    <span>{short}</span>
  );
}
