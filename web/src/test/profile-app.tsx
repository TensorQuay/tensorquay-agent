import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render } from "@testing-library/react";
import { useEffect, useState } from "react";

import type { User } from "../protocol/generated/api";
import type { ChoicesApi } from "../settings/profile-queries";
import { ProfilePage } from "../settings/ProfilePage";
import type { ProfilesApi } from "../settings/profiles";
import { ProfilesSettings } from "../settings/ProfilesSettings";

const LIST = "/settings/agent-profiles";

/**
 * Agent profiles and their pages as the app routes them, for tests (as showEnvironments): `router.push` and a click on
 * a link to one of them move between them; the test file mocks next/navigation with `router`, and next/link as a
 * plain link. A link click the page's leave guard holds never arrives here.
 */
export function showProfiles(
  api: ProfilesApi,
  choices: ChoicesApi,
  me: User,
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
    if (!rest) return <ProfilesSettings me={me} api={api} choices={choices} />;
    const [, id = "", action] = rest.split("/");
    return (
      <ProfilePage
        key={path}
        me={me}
        api={api}
        choices={choices}
        id={id === "new" ? null : decodeURIComponent(id)}
        duplicate={action === "duplicate"}
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
