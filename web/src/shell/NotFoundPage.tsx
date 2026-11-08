"use client";

import "../auth/catalogue";
import Link from "next/link";

import { useT } from "../i18n/I18nProvider";
import { APP_NAME } from "../lib/app";

import { TabTitle } from "./TabTitle";

/**
 * An address with no page (and `/design` on a release build): the product's own words in the reader's language and
 * theme, laid out as the sign-in pages are, with one way on. Before, Next.js's default 404 showed: always light, always
 * English, and a dead end (a pass over the pages before 17.0; WEB-U-381). Signed out, Go to your tasks leads to Sign
 * in.
 */
export function NotFoundPage() {
  const t = useT();
  return (
    <main id="main" className="login-page">
      <section className="auth-column">
        <TabTitle>{t("notFound.tab", { product: APP_NAME })}</TabTitle>
        <span
          className="brand-wordmark"
          role="img"
          aria-label={t("auth.wordmark")}
        />
        <header className="stack auth-heading">
          <h1 className="type-display">{t("notFound.title")}</h1>
          <p className="type-small muted">{t("notFound.body")}</p>
        </header>
        <Link href="/" className="button not-found-home" data-variant="primary">
          {t("notFound.home")}
        </Link>
      </section>
    </main>
  );
}
