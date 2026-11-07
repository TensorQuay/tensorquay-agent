import type { Page } from "@playwright/test";

/**
 * Every text the header's status held, from now on (WEB-U-415). A render the next one replaces in the same task is
 * never painted and no poll can read it, so a MutationObserver keeps each text node's old value and each added node's
 * text as well as the text after each batch: a render that lasts one commit is recorded every time.
 */
export async function recordTexts(page: Page) {
  await page.evaluate(() => {
    const status = document.querySelector('[aria-label="Run status"]');
    if (!status) throw new Error("No run status on the page");
    const seen: string[] = [];
    const note = (words: string | null) => {
      const trimmed = (words ?? "").trim();
      if (trimmed && seen.at(-1) !== trimmed) seen.push(trimmed);
    };
    note(status.textContent);
    new MutationObserver((records) => {
      for (const record of records) {
        for (const node of record.addedNodes) note(node.textContent);
        note(record.oldValue);
      }
      note(status.textContent);
    }).observe(status, {
      subtree: true,
      childList: true,
      characterData: true,
      characterDataOldValue: true,
    });
    (window as unknown as { headerTexts: string[] }).headerTexts = seen;
  });
  return () =>
    page.evaluate(
      () => (window as unknown as { headerTexts: string[] }).headerTexts,
    );
}
