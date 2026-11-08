import Link from "next/link";

/**
 * A table cell's one muted line (UI-GUIDELINES §7 Table, 17.3): cut with "…" where its column ends, its words whole in
 * its tooltip; the item's own page says them in full. With `href`, the line holds a link of its words, never cut: after
 * the reason `before` it (where to allow a host), or ahead of the words `after` it (where to add a missing key). A
 * space comes first, so the row's text and its name read "value note", never "valuenote".
 */
export function CellNote({
  children,
  href,
  before,
  after,
}: {
  children: string;
  href?: string;
  before?: string | null;
  after?: string | null;
}) {
  const words = [before, [children, after].filter(Boolean).join(" · ")]
    .filter(Boolean)
    .join(" ");
  return (
    <>
      {" "}
      <span className="type-small muted cell-note" title={words}>
        {href ? (
          <>
            {before ? <span>{`${before} `}</span> : null}
            <Link href={href} className="cell-note-link">
              {children}
            </Link>
            {after ? <span>{` · ${after}`}</span> : null}
          </>
        ) : (
          words
        )}
      </span>
    </>
  );
}
