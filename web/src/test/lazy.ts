import { act } from "@testing-library/react";

/**
 * Settles the conversation page's lazily loaded parts (the Markdown renderer, the tool card, the
 * Change panel's pickers, the files panel's Changes and preview views, and the sidebar's delete dialog)
 * inside act. A test awaits it after a render or
 * click that mounts them and then queries, so their one-time warm-up is awaited and never raced against the one-second
 * polling deadline of findBy and waitFor, which a loaded machine can miss (WEB-U-014).
 */
export async function settleLazy() {
  await act(async () => {
    await Promise.all([
      import("../transcript/AssistantMarkdown"),
      import("../transcript/ToolCard"),
      import("../conversations/ModelPicker"),
      import("../conversations/EnvironmentPicker"),
      import("../files/ChangesView"),
      import("../files/FilePreview"),
      import("../conversations/DeleteConversation"),
    ]);
  });
}
