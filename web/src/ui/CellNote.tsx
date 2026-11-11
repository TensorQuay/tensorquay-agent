import Link from "next/link";

import { ShortAndExact } from "./CompactCount";

/**
 * A table cell's one muted line (UI-GUIDELINES §7 Table, 17.3): cut with "…" where its column ends, its words whole in
 * its tooltip; the item's own page says them in full. A line that shows its figures short takes them `exact` too (§10,
 * WEB-U-461): its words without a link, or the words `after` its link. The short words show; the tooltip and screen
 * readers get the exact ones (`ShortAndExact`, untitled: the line's tooltip is its one). With `href`, the line holds a
 * link of its words, never cut: after the reason `before` it (where to allow a host), or ahead of the words `after` it
 * (where to add a missing key). A space comes first, so the row's text and its name read "value note", never
 * "valuenote".
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
            {after && exact ? (
              <ShortAndExact
                short={` · ${after}`}
                exact={` · ${exact}`}
                titled={false}
              />
            ) : after ? (
              <span>{` · ${after}`}</span>
            ) : null}
          </>
        ) : exact ? (
          <ShortAndExact short={words} exact={whole} titled={false} />
        ) : (
          words
        )}
      </span>
    </>
  );
}
