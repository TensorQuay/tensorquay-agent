import { useMemo } from "react";
import Markdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";

import { useT } from "../i18n/I18nProvider";
import type { Translate } from "../i18n/translate";

import { CodeBlock } from "./CodeBlock";
import "../i18n/deferred-catalogue";

const elements = [
  "p",
  "h1",
  "h2",
  "h3",
  "h4",
  "h5",
  "h6",
  "strong",
  "em",
  "del",
  "ul",
  "ol",
  "li",
  "blockquote",
  "code",
  "pre",
  "table",
  "thead",
  "tbody",
  "tr",
  "th",
  "td",
  "a",
  "hr",
  "br",
  "input",
  "img",
  // Only the streaming caret, which caretAfterLastWord adds: the model's own HTML always stays text.
  "span",
];
// A cell this short reads as one value ("2,124 B", "Ready"): it never wraps, so the table layout cannot split it
// (WEB-U-027). Longer cells wrap between words.
const SHORT_CELL = 16;
type CellNode = { type: string; value?: string; children?: CellNode[] };
const cellText = (node: CellNode): string =>
  node.type === "text"
    ? (node.value ?? "")
    : (node.children ?? []).map(cellText).join("");
const cellClass = (node: CellNode | undefined) =>
  node && cellText(node).trim().length <= SHORT_CELL
    ? "markdown-cell-short"
    : undefined;

/** Element renderers; the few words they add (image and table names) come from the reader's catalogue. */
function components(t: Translate): Components {
  return {
    a: ({ href, children }) =>
      href ? (
        <a href={href} target="_blank" rel="noopener noreferrer nofollow">
          {children}
        </a>
      ) : (
        <span>{children}</span>
      ),
    img: ({ alt, src }) => (
      <span>
        {t("markdown.imageSource", {
          alt: alt ?? t("markdown.image"),
          source: typeof src === "string" ? src : "",
        })}
      </span>
    ),
    input: ({ checked }) => (
      <input
        type="checkbox"
        checked={Boolean(checked)}
        disabled
        readOnly
        aria-label={t(checked ? "markdown.taskDone" : "markdown.taskOpen")}
      />
    ),
    code: ({ children }) => <code className="type-code">{children}</code>,
    // A labelled group, not a landmark: an answer can hold several tables (axe landmark-unique).
    table: ({ children }) => (
      <div
        className="markdown-table"
        tabIndex={0}
        role="group"
        aria-label={t("markdown.table")}
      >
        <table>{children}</table>
      </div>
    ),
    th: ({ node, ...props }) => <th {...props} className={cellClass(node)} />,
    td: ({ node, ...props }) => <td {...props} className={cellClass(node)} />,
    pre: ({ node }) => {
      const code = node?.children[0];
      if (code?.type !== "element") return null;
      const text = code.children
        .map((child) => (child.type === "text" ? child.value : ""))
        .join("");
      // An empty fence (the model opened a block and wrote nothing in it) is no code to copy: no box at all, while
      // the text around it stays (the c14b first hour, F9; WEB-U-296).
      if (!text.trim()) return null;
      const language =
        String(code.properties.className ?? "").match(
          /language-([^ ,]+)/,
        )?.[1] ?? "text";
      return <CodeBlock text={text} language={language} />;
    },
  };
}
function safeUrl(url: string, key: string): string {
  if (key === "src") return url; // Image destinations are rendered only as text.
  try {
    return ["http:", "https:", "mailto:"].includes(new URL(url).protocol)
      ? url
      : "";
  } catch {
    return "";
  }
}

/** The blocks whose last word the caret follows, and those it looks into for a later one. */
const TEXT_BLOCKS = new Set(["p", "h1", "h2", "h3", "h4", "h5", "h6", "li"]);
const CONTAINERS = new Set(["ul", "ol", "blockquote"]);
type Tree = {
  type: string;
  tagName?: string;
  properties?: Record<string, unknown>;
  children?: Tree[];
};

/**
 * Puts the streaming caret after the text's last word (UI-GUIDELINES §8, 17.3): in the last paragraph, heading or list
 * item, looking into a closing list or quote; after a closing code block or table, on a line of its own.
 */
function caretAfterLastWord() {
  return (tree: Tree) => {
    let into = tree;
    for (let node = tree; ;) {
      const last = node.children?.findLast((child) => child.type === "element");
      const tag = last?.tagName ?? "";
      if (!last || !(TEXT_BLOCKS.has(tag) || CONTAINERS.has(tag))) break;
      if (TEXT_BLOCKS.has(tag)) into = last;
      node = last;
    }
    (into.children ??= []).push({
      type: "element",
      tagName: "span",
      properties: { className: ["stream-caret"], ariaHidden: "true" },
      children: [],
    });
  };
}
const noPlugins: [] = [];

/**
 * Renders a partial answer through an element allow-list; HTML and images remain inert text. While `caret`, the text
 * is still arriving and the caret follows its last word.
 */
export function AssistantMarkdown({
  text,
  caret = false,
}: {
  text: string;
  caret?: boolean;
}) {
  const t = useT();
  const renderers = useMemo(() => components(t), [t]);
  return (
    <Markdown
      remarkPlugins={[remarkGfm]}
      rehypePlugins={caret ? [caretAfterLastWord] : noPlugins}
      allowedElements={elements}
      unwrapDisallowed
      components={renderers}
      urlTransform={safeUrl}
    >
      {text}
    </Markdown>
  );
}
