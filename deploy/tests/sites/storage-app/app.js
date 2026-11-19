// A to-do app that keeps its list in localStorage, as most small apps do. The preview's sandbox has no same-origin
// access, so the real storage throws; the preview's in-memory one lets it draw.
addEventListener("DOMContentLoaded", () => {
  const items = JSON.parse(localStorage.getItem("todos") || "[]");
  items.push("Buy coffee beans");
  localStorage.setItem("todos", JSON.stringify(items));
  sessionStorage.setItem("opened", "yes");
  const list = document.createElement("ul");
  list.id = "todos";
  for (const item of items)
    list.append(
      Object.assign(document.createElement("li"), { textContent: item }),
    );
  document.getElementById("root").append(list);
});
