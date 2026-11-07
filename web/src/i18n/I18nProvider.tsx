"use client";

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useState,
  type ReactNode,
} from "react";

import { CATALOGUES, LANGUAGES } from "./generated/languages";
import {
  catalogue,
  english,
  type Catalogue,
  negotiate,
  onReference,
  translator,
  type Translate,
} from "./translate";

const STORAGE_KEY = "tensorquay-language";

type Language = {
  language: string;
  t: Translate;
  source: Catalogue | null;
};
type Context = Omit<Language, "source"> & {
  setLanguage: (code: string) => void;
};

const I18nContext = createContext<Context>({
  language: "en",
  t: english,
  setLanguage: () => undefined,
});

/** The translator for the reader's language; English outside the provider. */
export function useT(): Translate {
  return useContext(I18nContext).t;
}

/** The current language, every shipped language (each in its own script) and a way to change it. */
export function useLanguage() {
  const { language, setLanguage } = useContext(I18nContext);
  return { language, setLanguage, languages: LANGUAGES };
}

async function load(code: string): Promise<Language | null> {
  if (code === "en") return { language: "en", t: english, source: null };
  const source = CATALOGUES[code];
  if (!source) return null;
  try {
    const loaded = (await source()) as { default?: unknown };
    const parsed = catalogue(loaded.default ?? loaded);
    return { language: code, t: translator(code, parsed), source: parsed };
  } catch {
    return null;
  }
}

function remembered(): string | null {
  try {
    return localStorage.getItem(STORAGE_KEY);
  } catch {
    return null;
  }
}

/**
 * Chooses the language after hydration (the server renders English): the remembered choice, else the browser's
 * preferred languages; loads only that catalogue and marks the document's language.
 */
export function I18nProvider({ children }: { children: ReactNode }) {
  const [current, setCurrent] = useState<Language>({
    language: "en",
    t: english,
    source: null,
  });
  // The failure copy loads on its first use (WEB-U-392): a new translator then renders its words.
  useEffect(
    () =>
      onReference(() =>
        setCurrent((shown) => ({
          ...shown,
          t: translator(shown.t.locale, shown.source),
        })),
      ),
    [],
  );
  useEffect(() => {
    const codes = LANGUAGES.map((entry) => entry.code as string);
    const stored = remembered();
    const choice =
      stored && codes.includes(stored)
        ? stored
        : negotiate(navigator.languages ?? [], codes);
    let active = true;
    void load(choice).then((next) => {
      if (!active || !next) return;
      document.documentElement.lang = next.language;
      setCurrent(next);
    });
    return () => {
      active = false;
    };
  }, []);
  const setLanguage = useCallback((code: string) => {
    try {
      localStorage.setItem(STORAGE_KEY, code);
    } catch {
      /* The current tab still changes when storage is unavailable. */
    }
    void load(code).then((next) => {
      if (!next) return;
      document.documentElement.lang = next.language;
      setCurrent(next);
    });
  }, []);
  return (
    <I18nContext.Provider
      value={{ language: current.language, t: current.t, setLanguage }}
    >
      {children}
    </I18nContext.Provider>
  );
}
