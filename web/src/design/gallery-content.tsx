"use client";

import { useRef, type ReactNode } from "react";

import { BrandLockup } from "../ui/BrandLockup";
import { Card } from "../ui/Card";
import { CellNote } from "../ui/CellNote";
import { CompactCount, ShortAndExact } from "../ui/CompactCount";
import { OverlayScrollbar } from "../ui/OverlayScrollbar";
import { StatusPill } from "../ui/StatusPill";
import { TableScroll } from "../ui/TableScroll";

import type { GallerySection } from "./gallery-types";

/** A scroller with the overlay bar beside it, in their positioned parent (as the task's transcript has them). */
function Scroller() {
  const target = useRef<HTMLDivElement>(null);
  return (
    <div className="gallery-scroll-frame">
      <div
        ref={target}
        className="gallery-scroll"
        role="region"
        tabIndex={0}
        aria-label="A long list"
      >
        {Array.from({ length: 24 }, (_, index) => (
          <p key={index}>Line {index + 1} of a long list</p>
        ))}
      </div>
      <OverlayScrollbar target={target} />
    </div>
  );
}

/** A field's width, as a table or a cell's note has in a page; a state alone shrinks to its content. */
function Wide({ children }: { children: ReactNode }) {
  return <div className="gallery-wide">{children}</div>;
}

/** A small table in its scroller, with a note in a cell. */
function Table({ long = false }: { long?: boolean }) {
  return (
    <TableScroll label="Shared keys">
      <table className="data-table">
        <thead>
          <tr>
            <th scope="col">Name</th>
            <th scope="col">Used by</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td>
              {long
                ? "MODELS_RUNTIME_EU_CENTRAL_1_EXAMPLE_COM_KEY"
                : "SEARCH_API_KEY"}
            </td>
            <td>
              <CellNote after="2 tasks">ai.example.org</CellNote>
            </td>
          </tr>
        </tbody>
      </table>
    </TableScroll>
  );
}

/** Surfaces, the lockup, numbers, notes, tables and scrollers (UI-GUIDELINES §7). */
export const CONTENT: GallerySection = {
  BrandLockup: {
    default: () => <BrandLockup />,
  },
  CompactCount: {
    small: () => <CompactCount value={842} locale="en" />,
    thousands: () => <CompactCount value={18_792} locale="en" />,
    millions: () => <CompactCount value={2_400_000} locale="en" />,
  },
  ShortAndExact: {
    default: () => <ShortAndExact short="18.8k" exact="18,792 tokens" />,
  },
  CellNote: {
    plain: () => (
      <Wide>
        <CellNote>ai.example.org</CellNote>
      </Wide>
    ),
    "with a link": () => (
      <Wide>
        <CellNote href="/settings/keys" before="Add it under" after="Shared">
          Keys
        </CellNote>
      </Wide>
    ),
    long: () => (
      <Wide>
        <CellNote after="every task of the team">
          models-runtime.eu-central-1.example.com
        </CellNote>
      </Wide>
    ),
  },
  TableScroll: {
    "a table": () => (
      <Wide>
        <Table />
      </Wide>
    ),
    "a long name": () => (
      <Wide>
        <Table long />
      </Wide>
    ),
  },
  OverlayScrollbar: {
    "beside its scroller": () => <Scroller />,
  },
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
