import { formatCompact, formatNumber } from "../i18n/message";

/**
 * A sentence shown short with its exact twin for screen readers (WEB-U-461): the short words show, titled with the
 * exact ones for hover; the exact words are what a screen reader reads.
 */
export function ShortAndExact({
  short,
  exact,
}: {
  short: string;
  exact: string;
}) {
  return (
    <>
      <span aria-hidden="true" title={exact}>
        {short}
      </span>
      <span className="visually-hidden">{exact}</span>
    </>
  );
}

/** A count in its language's short form (2.8M), its exact number on hover and to screen readers (WEB-U-461). */
export function CompactCount({
  value,
  locale,
}: {
  value: number;
  locale: string;
}) {
  return (
    <ShortAndExact
      short={formatCompact(value, locale)}
      exact={formatNumber(value, locale)}
    />
  );
}
