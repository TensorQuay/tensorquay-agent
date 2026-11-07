"use client";

/** A signed-in page's browser tab title: the page's own ("Tasks · TensorQuay Agent"), with no count in front. */
export function TabTitle({ children }: { children: string }) {
  return <title>{children}</title>;
}
