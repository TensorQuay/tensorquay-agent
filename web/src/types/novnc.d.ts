// The parts of noVNC's core (@novnc/novnc, MPL-2.0, unmodified) the Screen panel uses; it ships no types.
declare module "@novnc/novnc" {
  export default class RFB extends EventTarget {
    constructor(
      target: HTMLElement,
      urlOrChannel: string | object,
      options?: { shared?: boolean },
    );
    viewOnly: boolean;
    scaleViewport: boolean;
    resizeSession: boolean;
    clipViewport: boolean;
    focusOnClick: boolean;
    focus(): void;
    blur(): void;
    disconnect(): void;
  }
}

// noVNC's pointer capture, imported by its path only by the tests that stand in for a press on the picture (the
// package exports nothing but RFB).
declare module "*/@novnc/novnc/core/util/events.js" {
  export function setCapture(target: Element): void;
  export function releaseCapture(): void;
}
