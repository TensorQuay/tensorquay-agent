"use client";

import { useEffect, useLayoutEffect, useRef } from "react";

import { useT } from "../i18n/I18nProvider";

const INVALID = '[aria-invalid="true"]';

/**
 * Where a refused save needs fixing (WEB-U-110): placed by a form's Save, it waits for the form's next submit, then
 * moves the focus to the first field marked invalid (whether the page refused it at once or the gateway's answer marks
 * it later) and brings it into view, once per save; beside Save, a line counts the fields left to fix. The form
 * re-renders it with every change of its fields, so the count follows the person's fixes.
 */
export function FormRefusal() {
  const t = useT();
  const line = useRef<HTMLParagraphElement>(null);
  /** A save was asked for and its first invalid field has not had the focus yet. */
  const pending = useRef(false);
  /** A save was asked for at all: before one, nothing is counted. */
  const tried = useRef(false);
  useEffect(() => {
    const form = line.current?.closest("form");
    if (!form) return;
    const submitted = () => {
      pending.current = true;
      tried.current = true;
    };
    form.addEventListener("submit", submitted, true);
    return () => form.removeEventListener("submit", submitted, true);
  }, []);
  // After every render of the form: the invalid fields now, and the focus for a save just refused.
  useLayoutEffect(() => {
    const form = line.current?.closest("form");
    if (!form || !tried.current) return;
    const invalid = [...form.querySelectorAll<HTMLElement>(INVALID)];
    // Written here, after the form's render, so the count never waits for another render of its own.
    const words = invalid.length
      ? t("page.fixFields", { count: invalid.length })
      : "";
    if (line.current && line.current.textContent !== words)
      line.current.textContent = words;
    const first = invalid[0];
    if (pending.current && first) {
      pending.current = false;
      first.focus({ preventScroll: true });
      first.scrollIntoView?.({ block: "center" });
    }
  });
  return (
    <p
      ref={line}
      role="status"
      className="field-error type-small form-refusal"
    />
  );
}
