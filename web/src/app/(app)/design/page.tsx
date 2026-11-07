import { notFound } from "next/navigation";

import { AccountPatterns } from "../../../design/AccountPatterns";
import { ChannelPatterns } from "../../../design/ChannelPatterns";
import { ConversationPatterns } from "../../../design/ConversationPatterns";
import { DesignPanel } from "../../../design/DesignPanel";

export const metadata = { title: "Design reference · TensorQuay Agent" };

export default function Design() {
  if (
    process.env.NODE_ENV === "production" &&
    process.env.TQA_WEB_TEST_BUILD !== "1"
  )
    notFound();
  return (
    <>
      <header className="page-heading stack">
        <h1 className="type-display">Design reference</h1>
        <p className="muted">Tokens and components, in both themes.</p>
      </header>
      <div className="design-grid">
        <DesignPanel theme="light" />
        <DesignPanel theme="dark" />
        <ConversationPatterns theme="light" />
        <ConversationPatterns theme="dark" />
        <AccountPatterns theme="light" />
        <AccountPatterns theme="dark" />
        <ChannelPatterns theme="light" />
        <ChannelPatterns theme="dark" />
      </div>
    </>
  );
}
