// The shape of a bundler's output (vite build --base ./): a module entry that imports a hashed chunk, renders into
// #root and routes on the address's last segment, so the folder's index.html must answer an unknown path.
import { page } from "./chunk-b4e1d2.js";
const root = document.getElementById("root");
const route = location.pathname.endsWith("/") || location.pathname.endsWith("index.html")
  ? "home"
  : location.pathname.slice(location.pathname.lastIndexOf("/") + 1);
root.innerHTML = page(route);
