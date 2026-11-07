import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { I18nProvider, useLanguage, useT } from "./I18nProvider";

const load = vi.fn();
vi.mock("./generated/languages", () => ({
  LANGUAGES: [
    { code: "en", name: "English" },
    { code: "zh-Hans", name: "简体中文" },
  ],
  CATALOGUES: { "zh-Hans": () => load() as Promise<unknown> },
}));

function Probe() {
  const t = useT();
  const { language, setLanguage } = useLanguage();
  return (
    <>
      <p>{t("sidebar.collapse")}</p>
      <p data-testid="language">{language}</p>
      <button onClick={() => setLanguage("en")}>English</button>
    </>
  );
}
function languages(list: string[]) {
  vi.spyOn(navigator, "languages", "get").mockReturnValue(list);
}
beforeEach(() => {
  localStorage.clear();
  load.mockReset().mockResolvedValue({
    default: {
      language: "简体中文",
      messages: { "sidebar.collapse": "收起侧边栏" },
    },
  });
});
afterEach(() => {
  vi.restoreAllMocks();
  document.documentElement.lang = "en";
});

// WEB-U-019: the browser language picks the default, the choice persists per browser, and a failed load keeps English.
describe("WEB-U-019 language provider", () => {
  it("follows the browser language and marks the document", async () => {
    languages(["zh-CN", "en"]);
    render(
      <I18nProvider>
        <Probe />
      </I18nProvider>,
    );
    expect(await screen.findByText("收起侧边栏")).toBeVisible();
    expect(document.documentElement.lang).toBe("zh-Hans");
  });

  it("prefers the remembered choice and remembers a new one", async () => {
    languages(["zh-CN"]);
    localStorage.setItem("tensorquay-language", "en");
    render(
      <I18nProvider>
        <Probe />
      </I18nProvider>,
    );
    await waitFor(() =>
      expect(screen.getByTestId("language")).toHaveTextContent("en"),
    );
    expect(load).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "English" }));
    expect(localStorage.getItem("tensorquay-language")).toBe("en");
  });

  it("keeps English when a catalogue cannot load", async () => {
    languages(["zh-CN"]);
    load.mockRejectedValue(new Error("offline"));
    render(
      <I18nProvider>
        <Probe />
      </I18nProvider>,
    );
    await act(async () => {
      await Promise.resolve();
    });
    expect(screen.getByText("Collapse sidebar")).toBeVisible();
    expect(screen.getByTestId("language")).toHaveTextContent("en");
  });

  it("gives English outside the provider", () => {
    render(<Probe />);
    expect(screen.getByText("Collapse sidebar")).toBeVisible();
  });
});
