"use client";

import { useEffect, useRef, useState, type CSSProperties } from "react";

import { lastInputWasKeyboard } from "./input-modality";
import { useDismiss } from "./use-dismiss";

/**
 * The UI standard's In-place edit: the text itself becomes the field, at its own font and place, all of it selected.
 * Enter, a click elsewhere or moving focus away saves; Esc cancels; an empty or unchanged text changes nothing. It has
 * no visible label (its name is `label`) and no buttons. A save that fails keeps the text, to try again; the caller
 * says why. Its focus ring shows only when the keyboard opened it; opened by a click it is the text and its underline.
 */
export function InlineEdit({
  value,
  label,
  maxLength = 200,
  className,
  style,
  onCommit,
  onCancel,
}: {
  value: string;
  label: string;
  maxLength?: number;
  /** The text's own type class, so the field reads as the text it replaces. */
  className?: string | undefined;
  style?: CSSProperties | undefined;
  /** Saves the trimmed text; false keeps editing. */
  onCommit: (value: string) => Promise<boolean> | boolean;
  /** Leaves editing with the old text. */
  onCancel: () => void;
}) {
  const field = useRef<HTMLInputElement>(null);
  const [draft, setDraft] = useState(value);
  const [saving, setSaving] = useState(false);
  // Read once, as it opens: the key or click that opened it (WEB-U-114).
  const [viaKeyboard] = useState(lastInputWasKeyboard);
  // Set once the edit ends, so Enter, a click elsewhere and the blur that follows it act once.
  const settled = useRef(false);
  useEffect(() => {
    field.current?.focus();
    field.current?.select();
  }, []);
  function cancel() {
    if (settled.current) return;
    settled.current = true;
    onCancel();
  }
  async function commit() {
    if (settled.current) return;
    const text = draft.trim();
    if (!text || text === value.trim()) {
      cancel();
      return;
    }
    settled.current = true;
    setSaving(true);
    const saved = await onCommit(text);
    setSaving(false);
    if (!saved) settled.current = false;
  }
  useDismiss(field, {
    onEscape: cancel,
    onOutside: () => void commit(),
  });
  return (
    <input
      ref={field}
      className={["inline-edit", className].filter(Boolean).join(" ")}
      style={style}
      aria-label={label}
      aria-busy={saving || undefined}
      data-keyboard={viaKeyboard || undefined}
      readOnly={saving}
      value={draft}
      maxLength={maxLength}
      spellCheck={false}
      onChange={(event) => setDraft(event.target.value)}
      onKeyDown={(event) => {
        if (event.key !== "Enter") return;
        event.preventDefault();
        void commit();
      }}
      onBlur={() => void commit()}
    />
  );
}
