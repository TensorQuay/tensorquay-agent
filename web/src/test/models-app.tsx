import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render } from "@testing-library/react";
import { useEffect, useState } from "react";

import type { User } from "../protocol/generated/api";
import type { RoutesApi } from "../settings/admin/routes";
import { ModelsSettings } from "../settings/ModelsSettings";
import { RoutePage } from "../settings/RoutePage";

import { LIMITS } from "./user";

export const ADMIN: User = {
  id: "0195d8c0-0000-7000-8000-000000000001",
  email: "admin@example.invalid",
  role: "admin",
  internet: "none",
  limits: LIMITS,
  features: { agent_teams: false },
};

const LIST = "/settings/models";

/**
 * The Models list and its route pages as the app routes them, for tests: `router.push` and a click on a link to one of
 * them move between them, as the app's own navigation would (the test file mocks next/navigation with `router`, and
 * next/link as a plain link). A link click the page's leave guard holds never arrives here.
 */
export function showModels(
  api: RoutesApi,
  router: { push: (href: string) => void },
  start = LIST,
) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const at = { path: start };
  function App() {
    const [path, setPath] = useState(start);
    useEffect(() => {
      router.push = (href) => {
        at.path = href;
        setPath(href);
      };
      function click(event: MouseEvent) {
        const link =
          event.target instanceof Element
            ? event.target.closest<HTMLAnchorElement>("a[href]")
            : null;
        if (!link || event.defaultPrevented) return;
        const url = new URL(link.href, location.href);
        if (!url.pathname.startsWith(LIST)) return;
        event.preventDefault();
        router.push(url.pathname);
      }
      document.addEventListener("click", click);
      return () => document.removeEventListener("click", click);
    }, []);
    const rest = path.slice(LIST.length);
    if (!rest) return <ModelsSettings me={ADMIN} api={api} />;
    return (
      <RoutePage
        key={path}
        me={ADMIN}
        api={api}
        name={rest === "/new" ? null : decodeURIComponent(rest.slice(1))}
      />
    );
  }
  render(
    <QueryClientProvider client={cache}>
      <App />
    </QueryClientProvider>,
  );
  return at;
}
