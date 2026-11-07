/**
 * Snapshots are published on the next animation frame, or after this long if no frame comes first: a starved host or
 * an occluded window can hold frames back for seconds, and the page still shows what arrived (TEST-PLAN §19a, WEB-U-278).
 * A module of its own, importing nothing, so the browser specs can read it without loading the app's protocol code.
 */
export const PUBLISH_FALLBACK_MS = 250;
