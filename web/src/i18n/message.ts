/**
 * The ICU MessageFormat subset the catalogues use (UI-GUIDELINES §11b): `{name}`, `{name, number}`,
 * `{name, plural, =0 {…} one {…} other {…}}` with `#`, `{name, select, a {…} other {…}}`, and apostrophe quoting
 * (`''` for an apostrophe, `'{'` for a literal brace). Plural categories come from `Intl.PluralRules` for the language.
 */
export type MessageValues = Readonly<Record<string, string | number>>;

type Node =
  | string
  | { argument: string }
  | { argument: string; number: true }
  | {
      argument: string;
      kind: "plural" | "select";
      options: Readonly<Record<string, Node[]>>;
    };

/**
 * UI-GUIDELINES §10: numbers group thousands with U+202F NARROW NO-BREAK SPACE in every language: as narrow as a thin
 * space, but a line never breaks inside a number (WEB-U-354).
 */
const GROUP_SPACE = "\u202f";
const MAX_PATTERN_LENGTH = 2000;

class Parser {
  private index = 0;
  constructor(private readonly source: string) {}

  message(inPlural: boolean, closing: boolean): Node[] {
    const nodes: Node[] = [];
    let text = "";
    while (this.index < this.source.length) {
      const character = this.source[this.index] ?? "";
      if (character === "'") {
        text += this.quoted();
        continue;
      }
      if (character === "}") {
        // copy-lint: not user copy (internal diagnostic)
        if (!closing) throw new Error(`Unexpected } at ${this.index}`);
        break;
      }
      if (character === "{") {
        if (text) nodes.push(text);
        text = "";
        this.index += 1;
        nodes.push(this.argument());
        continue;
      }
      if (character === "#" && inPlural) {
        if (text) nodes.push(text);
        text = "";
        nodes.push({ argument: "#", number: true });
        this.index += 1;
        continue;
      }
      text += character;
      this.index += 1;
    }
    if (closing && this.source[this.index] !== "}")
      // copy-lint: not user copy (internal diagnostic)
      throw new Error("Unclosed option in message");
    if (text) nodes.push(text);
    return nodes;
  }

  private quoted(): string {
    const next = this.source[this.index + 1];
    if (next === "'") {
      this.index += 2;
      return "'";
    }
    if (next !== "{" && next !== "}" && next !== "#") {
      this.index += 1;
      return "'";
    }
    const end = this.source.indexOf("'", this.index + 1);
    // copy-lint: not user copy (internal diagnostic)
    if (end < 0) throw new Error("Unclosed quote in message");
    const literal = this.source.slice(this.index + 1, end);
    this.index = end + 1;
    return literal;
  }

  private argument(): Node {
    const end = this.source.indexOf("}", this.index);
    const comma = this.source.indexOf(",", this.index);
    // copy-lint: not user copy (internal diagnostic)
    if (end < 0) throw new Error("Unclosed argument in message");
    if (comma < 0 || comma > end) {
      const argument = this.source.slice(this.index, end).trim();
      this.index = end + 1;
      return { argument };
    }
    const argument = this.source.slice(this.index, comma).trim();
    this.index = comma + 1;
    const typeEnd = this.source.slice(this.index).search(/[,}]/) + this.index;
    const type = this.source.slice(this.index, typeEnd).trim();
    this.index = typeEnd;
    if (type === "number") {
      this.expect("}");
      return { argument, number: true };
    }
    if (type !== "plural" && type !== "select")
      // copy-lint: not user copy (internal diagnostic)
      throw new Error(`Unsupported argument type ${type}`);
    this.expect(",");
    const options: Record<string, Node[]> = {};
    for (;;) {
      this.skipSpace();
      if (this.source[this.index] === "}") {
        this.index += 1;
        break;
      }
      const keyEnd = this.source.indexOf("{", this.index);
      // copy-lint: not user copy (internal diagnostic)
      if (keyEnd < 0) throw new Error("Unclosed option list in message");
      const key = this.source.slice(this.index, keyEnd).trim();
      this.index = keyEnd + 1;
      options[key] = this.message(type === "plural", true);
      this.index += 1;
    }
    if (!options.other)
      // copy-lint: not user copy (internal diagnostic)
      throw new Error(`The ${type} argument ${argument} needs an other option`);
    return { argument, kind: type, options };
  }

  private expect(character: string) {
    this.skipSpace();
    if (this.source[this.index] !== character)
      // copy-lint: not user copy (internal diagnostic)
      throw new Error(`Expected ${character} at ${this.index}`);
    this.index += 1;
  }

  private skipSpace() {
    while (/\s/.test(this.source[this.index] ?? "")) this.index += 1;
  }
}

const parsed = new Map<string, Node[]>();

function parse(pattern: string): Node[] {
  const cached = parsed.get(pattern);
  if (cached) return cached;
  if (pattern.length > MAX_PATTERN_LENGTH)
    // copy-lint: not user copy (internal diagnostic)
    throw new Error("Message pattern is too long");
  const nodes = new Parser(pattern).message(false, false);
  parsed.set(pattern, nodes);
  return nodes;
}

// Intl formatters are costly to build; one per language is enough.
const numberFormats = new Map<string, Intl.NumberFormat>();
const pluralRules = new Map<string, Intl.PluralRules>();

function numberFormat(locale: string): Intl.NumberFormat {
  let format = numberFormats.get(locale);
  if (!format) {
    format = new Intl.NumberFormat(locale);
    numberFormats.set(locale, format);
  }
  return format;
}

export type DurationUnit = "hour" | "minute" | "second" | "millisecond";
type SizeUnit = "byte" | "kilobyte" | "megabyte" | "gigabyte";

/**
 * English keeps the SI symbol the product writes ("5 s", UI-GUIDELINES §7) where Intl's short form says "sec", and
 * English and Japanese write "B" where Intl says "byte"; every other language uses its own short unit. Simplified
 * Chinese spaces a number from its unit (UI-GUIDELINES §11b), where Intl attaches 秒 and 毫秒 ("5秒"): the space is
 * part of the symbol, as Intl gives no literal part to replace.
 */
const UNIT_OVERRIDES: Readonly<
  Record<string, Partial<Record<DurationUnit | SizeUnit, string>>>
> = {
  // "h" and "min" as the team's time left says them (WEB-U-395), not Intl's "hr".
  en: { hour: "h", minute: "min", second: "s", byte: "B" },
  ja: { byte: "B" },
  zh: { hour: " 小时", minute: " 分钟", second: " 秒", millisecond: " 毫秒" },
};
const unitFormats = new Map<string, Intl.NumberFormat>();

/** A duration in the reader's language: its own short unit, at most two decimals, narrow no-break space thousands (§10). */
export function formatDuration(
  value: number,
  unit: DurationUnit,
  locale: string,
): string {
  return formatUnit(value, unit, locale, 2);
}

const SIZE_UNITS: readonly SizeUnit[] = [
  "byte",
  "kilobyte",
  "megabyte",
  "gigabyte",
];

/** A file size in binary steps (1 kB = 1024 bytes, the size limits' own base), at most one decimal. */
export function formatSize(bytes: number, locale: string): string {
  let step = 0;
  let value = Math.max(0, bytes);
  while (value >= 1024 && step < SIZE_UNITS.length - 1) {
    value /= 1024;
    step++;
  }
  return formatUnit(value, SIZE_UNITS[step] ?? "byte", locale, 1);
}

/**
 * A size set in MiB (an environment's memory, a disk), written as every other size is (WEB-U-185) and read back as it
 * was set: the GB fields take quarters, so a whole number of quarter-GBs reads in GB ("4 GB", "0.5 GB", never "4.3 GB"
 * or "512 MB"); anything else as `formatSize` writes it, in the same binary steps.
 */
export function formatMib(mib: number, locale: string): string {
  return mib >= 256 && mib % 256 === 0
    ? formatUnit(mib / 1024, "gigabyte", locale, 2)
    : formatSize(mib * 1024 * 1024, locale);
}

function formatUnit(
  value: number,
  unit: DurationUnit | SizeUnit,
  locale: string,
  digits: number,
): string {
  const key = `${locale} ${unit} ${digits}`;
  let format = unitFormats.get(key);
  if (!format) {
    format = new Intl.NumberFormat(locale, {
      style: "unit",
      unit,
      unitDisplay: "short",
      maximumFractionDigits: digits,
    });
    unitFormats.set(key, format);
  }
  const language = format.resolvedOptions().locale.split("-")[0] ?? "";
  const symbol = UNIT_OVERRIDES[language]?.[unit];
  return format
    .formatToParts(value)
    .map((part) =>
      part.type === "group"
        ? GROUP_SPACE
        : part.type === "unit" && symbol
          ? symbol
          : part.value,
    )
    .join("");
}

function pluralRule(locale: string): Intl.PluralRules {
  let rule = pluralRules.get(locale);
  if (!rule) {
    rule = new Intl.PluralRules(locale);
    pluralRules.set(locale, rule);
  }
  return rule;
}

const compactFormats = new Map<string, Intl.NumberFormat>();

/**
 * Formats a count in its language's short form (WEB-U-461): 2.8M, 282.5万, 2,8 M, with one decimal at most. Token
 * counts read this way; the exact number stays one step away (CompactCount).
 */
export function formatCompact(value: number, locale: string): string {
  let format = compactFormats.get(locale);
  if (!format) {
    format = new Intl.NumberFormat(locale, {
      notation: "compact",
      maximumFractionDigits: 1,
    });
    compactFormats.set(locale, format);
  }
  return format.format(value);
}

/** Formats a number for the language with narrow no-break space thousands grouping (§10, WEB-U-354). */
export function formatNumber(value: number, locale: string): string {
  return numberFormat(locale)
    .formatToParts(value)
    .map((part) => (part.type === "group" ? GROUP_SPACE : part.value))
    .join("");
}

function render(
  nodes: readonly Node[],
  values: MessageValues,
  locale: string,
  count: number | null,
): string {
  return nodes
    .map((node) => {
      if (typeof node === "string") return node;
      if (node.argument === "#" && count !== null)
        return formatNumber(count, locale);
      const value = values[node.argument];
      if (value === undefined) return `{${node.argument}}`;
      if ("number" in node)
        return typeof value === "number"
          ? formatNumber(value, locale)
          : String(value);
      if (!("kind" in node)) return String(value);
      if (node.kind === "select") {
        const chosen = node.options[String(value)] ?? node.options.other ?? [];
        return render(chosen, values, locale, count);
      }
      const amount = Number(value);
      const exact = node.options[`=${amount}`];
      const category = pluralRule(locale).select(amount);
      const chosen =
        exact ?? node.options[category] ?? node.options.other ?? [];
      return render(chosen, values, locale, amount);
    })
    .join("");
}

/** Formats one catalogue message for a language; malformed patterns throw so tests catch them. */
export function formatMessage(
  pattern: string,
  values: MessageValues,
  locale: string,
): string {
  return render(parse(pattern), values, locale, null);
}

/** The argument names a pattern uses (plural `#` excluded), from its parse tree. */
export function argumentNames(pattern: string): Set<string> {
  const names = new Set<string>();
  const walk = (nodes: readonly Node[]) => {
    for (const node of nodes) {
      if (typeof node === "string" || node.argument === "#") continue;
      names.add(node.argument);
      if ("options" in node)
        for (const option of Object.values(node.options)) walk(option);
    }
  };
  walk(parse(pattern));
  return names;
}
