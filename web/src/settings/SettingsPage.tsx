"use client";

import { ArrowLeft } from "lucide-react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import {
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type FormEvent,
  type ReactNode,
} from "react";

import { useT } from "../i18n/I18nProvider";
import { Button } from "../ui/Button";
import { Dialog } from "../ui/Dialog";
import { FormRefusal } from "../ui/FormRefusal";
import { Icon } from "../ui/Icon";

/**
 * A new item's ID made from its display name (the UI standard's Settings pages, WEB-U-097): it follows the display name
 * until typed by hand, and Change opens its field with the focus in it.
 */
export function useDerivedId() {
  const [byHand, setByHand] = useState(false);
  const [open, setOpen] = useState(false);
  const field = useId();
  const focus = useRef(false);
  useEffect(() => {
    if (!open || !focus.current) return;
    focus.current = false;
    document.getElementById(field)?.focus();
  }, [open, field]);
  return {
    /** Typed by hand: the display name no longer moves it. */
    byHand,
    /** Its field is open. */
    open,
    /** The field's element id, for the ID input. */
    field,
    typed: () => {
      setByHand(true);
      setOpen(true);
    },
    change: () => {
      focus.current = true;
      setOpen(true);
    },
  };
}

/** The quiet line under the display name: "ID: data-science · Change" (WEB-U-097). */
export function IdLine({ id, onChange }: { id: string; onChange: () => void }) {
  const t = useT();
  return (
    <p className="field-note type-small muted">
      {t("page.idLine", { id })}
      {" · "}
      <button
        type="button"
        className="inline-action"
        aria-label={t("page.changeIdName")}
        onClick={onChange}
      >
        {t("page.changeId")}
      </button>
    </p>
  );
}

/**
 * The UI standard's Page, for creating or editing anything with sections (WEB-U-090): its own address, the way back at
 * the top, the title, sections in one column (each label above its field; lists the whole width, WEB-U-131), and
 * Save and Cancel pinned at the bottom. Leaving with unsaved changes asks first: Cancel, the way back, any link on the
 * page (the Settings menu, the sidebar), and the browser's own close or reload.
 */
export function SettingsPage({
  back,
  title,
  dirty,
  pending,
  failure,
  onSave,
  readOnly = false,
  children,
}: {
  back: { href: string; label: string };
  title: string;
  /** Whether anything differs from what was loaded. */
  dirty: boolean;
  pending: boolean;
  /** Why the last save didn't happen, in plain words. */
  failure: string | null;
  onSave: () => void;
  /** Nothing on it can be changed now (the page says why): no Save, and Close goes back. */
  readOnly?: boolean;
  children: ReactNode;
}) {
  const t = useT();
  const leave = useUnsavedGuard(dirty);
  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!readOnly) onSave();
  }
  return (
    <form className="stack settings-form-page" noValidate onSubmit={submit}>
      <Link href={back.href} className="back-link type-small">
        <Icon glyph={ArrowLeft} size={16} />
        {back.label}
      </Link>
      <h2 className="type-heading">{title}</h2>
      {children}
      <div
        role="group"
        aria-label={t("page.actions")}
        className="settings-form-footer"
      >
        {failure && (
          <p role="alert" className="field-error type-small">
            {failure}
          </p>
        )}
        {/* A refused save shows where: the first field to fix has the focus, and this line counts them (WEB-U-110). */}
        <FormRefusal />
        <p role="status" className="type-small muted settings-form-unsaved">
          {dirty ? t("page.unsaved") : ""}
        </p>
        <Button
          variant="secondary"
          type="button"
          disabled={pending}
          onClick={() => leave.to(back.href)}
        >
          {t(readOnly ? "common.close" : "common.cancel")}
        </Button>
        {!readOnly && (
          <Button type="submit" loading={pending}>
            {t("page.save")}
          </Button>
        )}
      </div>
      {leave.dialog}
    </form>
  );
}

/** One section of a page's fields, under its heading. */
export function PageSection({
  title,
  children,
}: {
  title: string;
  children: ReactNode;
}) {
  const id = useId();
  return (
    <section className="page-section" aria-labelledby={id}>
      <h3 id={id} className="type-body-strong page-section-title">
        {title}
      </h3>
      {children}
    </section>
  );
}

/**
 * Asks before unsaved changes are left: an in-app link's click is held (heard on the document before the link's own
 * handler), Cancel asks the same way, and the browser's close or reload gets its own prompt. A link opened elsewhere (a
 * new tab, a download) never asks: the page stays.
 */
function useUnsavedGuard(dirty: boolean) {
  const t = useT();
  const router = useRouter();
  const [asking, setAsking] = useState<string | null>(null);
  const changed = useRef(dirty);
  useEffect(() => {
    changed.current = dirty;
  }, [dirty]);
  // Its listeners go with the page, in the same commit (a layout effect's clean-up, not a later one): a save that leaves
  // from a promise shows the next page first, and a click there must never meet a guard whose page is gone.
  useLayoutEffect(() => {
    function click(event: MouseEvent) {
      if (!changed.current || event.defaultPrevented || event.button !== 0)
        return;
      if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey)
        return;
      const link =
        event.target instanceof Element
          ? event.target.closest<HTMLAnchorElement>("a[href]")
          : null;
      if (!link || link.target === "_blank" || link.hasAttribute("download"))
        return;
      const url = new URL(link.href, location.href);
      if (url.origin !== location.origin) return;
      event.preventDefault();
      event.stopPropagation();
      setAsking(`${url.pathname}${url.search}${url.hash}`);
    }
    function unload(event: BeforeUnloadEvent) {
      if (!changed.current) return;
      event.preventDefault();
      // Older browsers ask only when a return value is set.
      event.returnValue = "";
    }
    document.addEventListener("click", click, true);
    window.addEventListener("beforeunload", unload);
    return () => {
      document.removeEventListener("click", click, true);
      window.removeEventListener("beforeunload", unload);
    };
  }, []);
  return {
    /** Goes to `href`, asking first when there are unsaved changes. */
    to(href: string) {
      if (changed.current) setAsking(href);
      else router.push(href);
    },
    dialog: asking && (
      <Dialog
        open
        title={t("page.leaveTitle")}
        onClose={() => setAsking(null)}
        actions={
          <>
            <Button variant="secondary" onClick={() => setAsking(null)}>
              {t("page.keepEditing")}
            </Button>
            <Button
              variant="danger"
              onClick={() => {
                const href = asking;
                setAsking(null);
                changed.current = false;
                router.push(href);
              }}
            >
              {t("page.leave")}
            </Button>
          </>
        }
      >
        <p>{t("page.leaveBody")}</p>
      </Dialog>
    ),
  };
}
