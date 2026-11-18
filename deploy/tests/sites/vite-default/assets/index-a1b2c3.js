// The shape of a default Vite build's entry: it draws into #root, and an image imported in the script is a
// root-path string the preview can't move (scripts are never rewritten).
const root = document.getElementById("root");
const title = document.createElement("h1");
title.id = "title";
title.textContent = "Built app";
const logo = document.createElement("img");
logo.alt = "";
logo.src = "/assets/logo-9f8e7d.png";
root.append(title, logo);
