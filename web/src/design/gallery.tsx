import { CONTENT } from "./gallery-content";
import { CONTROLS } from "./gallery-controls";
import { FEEDBACK } from "./gallery-feedback";
import type { GallerySection } from "./gallery-types";

/**
 * Every component in web/src/ui and its states (WEB-U-510): /design draws them all, in both themes, and the coverage
 * test fails for a component with no entry here and no exemption below.
 */
export const GALLERY: GallerySection = { ...CONTROLS, ...FEEDBACK, ...CONTENT };

/** Components drawn only inside another, each with why (four words or more). */
export const EXEMPT: Readonly<Record<string, string>> = {
  ActionMenuSurface: "the open ActionMenu, loaded when it opens",
  PickerSurface: "the open MenuButton list, loaded when it opens",
  PanelBoundary: "an error boundary: what it draws is PanelFailure",
};
