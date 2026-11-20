// An app that stops on an error before drawing: the preview gives it no router advice, which wouldn't help.
addEventListener("DOMContentLoaded", () => {
  throw new Error("the app's own bug");
});
