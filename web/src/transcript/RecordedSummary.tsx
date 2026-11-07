/** Marks the recorded duration separately from the stable work counts. */
export function RecordedSummary({ text }: { text: string }) {
  const match = /\b\d+(?:\.\d+)? (?:ms|s)\b/.exec(text);
  if (!match) return text;
  return (
    <>
      {text.slice(0, match.index)}
      <span className="recorded-duration">{match[0]}</span>
      {text.slice(match.index + match[0].length)}
    </>
  );
}
