// A to-do app as most small apps are: its list in localStorage, and a form whose submit handler adds an item. The
// preview's sandbox has no same-origin access (the real storage throws) and no forms (the browser never fires submit);
// the preview lends an in-memory storage and fires the submit itself, so the app draws and adds.
addEventListener("DOMContentLoaded", () => {
  const root = document.getElementById("root");
  const form = Object.assign(document.createElement("form"), { id: "add" });
  const input = Object.assign(document.createElement("input"), { id: "item" });
  const button = Object.assign(document.createElement("button"), {
    textContent: "Add",
  });
  const list = Object.assign(document.createElement("ul"), { id: "todos" });
  form.append(input, button);
  root.append(form, list);
  const draw = () => {
    const items = JSON.parse(localStorage.getItem("todos") || "[]");
    list.replaceChildren(
      ...items.map((item) =>
        Object.assign(document.createElement("li"), { textContent: item }),
      ),
    );
  };
  form.addEventListener("submit", (event) => {
    event.preventDefault();
    const items = JSON.parse(localStorage.getItem("todos") || "[]");
    items.push(input.value);
    localStorage.setItem("todos", JSON.stringify(items));
    input.value = "";
    draw();
  });
  if (!localStorage.getItem("todos"))
    localStorage.setItem("todos", JSON.stringify(["Buy coffee beans"]));
  sessionStorage.setItem("opened", "yes");
  draw();
});
