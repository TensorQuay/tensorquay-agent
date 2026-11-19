"use client";

import { Card } from "../ui/Card";
import { StatusPill } from "../ui/StatusPill";

import type { GallerySection } from "./gallery-types";

/** Surfaces, names and numbers, tables (UI-GUIDELINES §7). */
export const CONTENT: GallerySection = {
  Card: {
    default: () => (
      <Card className="stack">
        <p className="type-body-strong">Quarterly review</p>
        <p className="type-small muted">
          A flat surface for related information.
        </p>
        <StatusPill status="idle" />
      </Card>
    ),
  },
};
