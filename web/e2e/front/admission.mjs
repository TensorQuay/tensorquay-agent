const stages = [
  { state: "queued", queue: { ahead: 2 } },
  { state: "queued", queue: { ahead: 1 } },
  { state: "queued", queue: { ahead: 0 } },
  { state: "creating" },
];

/** The fake scheduler advances only under test control; no wall-clock race changes the position. */
export function queueMessage(conversation, text) {
  if (text !== "Wait for a free slot") return false;
  conversation.admission = { stage: 0, text };
  conversation.running = true;
  return true;
}

/** Returns the current transient snapshot; it is never part of durable history. */
export function admissionStatus(conversation) {
  return conversation.admission ? stages[conversation.admission.stage] : null;
}

/** Returns the unchanged accepted message only when admission completes. */
export function advanceQueue(conversation) {
  const current = conversation.admission;
  if (++current.stage < stages.length) return null;
  conversation.admission = null;
  return current.text;
}
