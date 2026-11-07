"use client";

import { useRef, useState, type DragEvent } from "react";

const carriesFiles = (event: DragEvent) =>
  Array.from(event.dataTransfer?.types ?? []).includes("Files"); // copy-lint: not user copy (a DataTransfer type)

/** Drag-and-drop of files onto one element; `dragging` is true while files hover over it. */
export function useDrop(onFiles: ((files: File[]) => void) | null) {
  const [dragging, setDragging] = useState(false);
  const depth = useRef(0);
  if (!onFiles) return { dragging: false, handlers: {} };
  return {
    dragging,
    handlers: {
      onDragEnter: (event: DragEvent) => {
        if (!carriesFiles(event)) return;
        event.preventDefault();
        depth.current++;
        setDragging(true);
      },
      onDragOver: (event: DragEvent) => {
        if (!carriesFiles(event)) return;
        event.preventDefault();
        setDragging(true);
      },
      onDragLeave: (event: DragEvent) => {
        if (!carriesFiles(event)) return;
        depth.current = Math.max(0, depth.current - 1);
        if (depth.current === 0) setDragging(false);
      },
      onDrop: (event: DragEvent) => {
        if (!carriesFiles(event)) return;
        event.preventDefault();
        depth.current = 0;
        setDragging(false);
        onFiles(Array.from(event.dataTransfer.files));
      },
    },
  };
}
