import type { ReactNode } from "react";

/** One component's states in the gallery: each named state draws it so (WEB-U-510). */
export type GalleryStates = Readonly<Record<string, () => ReactNode>>;
export type GallerySection = Readonly<Record<string, GalleryStates>>;
