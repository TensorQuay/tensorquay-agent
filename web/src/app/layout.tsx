import type { Metadata } from "next";
import { headers } from "next/headers";
import type { ReactNode } from "react";

import { SIDEBAR_SCRIPT } from "../shell/sidebar";
import { SkipLink } from "../shell/SkipLink";
import { THEME_SCRIPT } from "../shell/theme";
import { Toast } from "../ui/Toast";

import Providers from "./providers";
import "./globals.css";
import "./conversation.css";
import "./files.css";
import "./screen.css";
import "./markdown.css";
import "./settings.css";
import "./workspaces.css";
import "./channels.css";
import "./teams.css";

export const metadata: Metadata = {
  icons: { icon: "/brand/logo.svg", apple: "/brand/logo.svg" },
};

export default async function RootLayout({
  children,
}: Readonly<{ children: ReactNode }>) {
  // The proxy's nonce for this page (src/proxy.ts); the only request value the server reads (server-boundary.mjs).
  const nonce = (await headers()).get("x-nonce") ?? undefined;
  return (
    <html lang="en" suppressHydrationWarning>
      <head>
        <script nonce={nonce}>{THEME_SCRIPT}</script>
        <script nonce={nonce}>{SIDEBAR_SCRIPT}</script>
      </head>
      <body>
        <Providers>
          <SkipLink />
          {children}
          <Toast />
        </Providers>
      </body>
    </html>
  );
}
