"use client";

import { ArrowUp, Pencil, Plus, Search, Trash2 } from "lucide-react";
import { useId, useState } from "react";

import { ActionMenu } from "../ui/ActionMenu";
import { Button, type ButtonVariant } from "../ui/Button";
import { CheckRow } from "../ui/CheckRow";
import { Field } from "../ui/Field";
import { Icon } from "../ui/Icon";
import { IconAction } from "../ui/IconAction";
import { InlineEdit } from "../ui/InlineEdit";
import { Input } from "../ui/Input";
import { MenuButton } from "../ui/MenuButton";
import { Select } from "../ui/Select";
import { TabList, tabPanelIds } from "../ui/Tabs";
import { Textarea } from "../ui/Textarea";

import type { GallerySection } from "./gallery-types";

const none = () => undefined;
const LONG =
  "Quarterly revenue by region, with the churn of every tier and the forecast for March";

function buttons(variant: ButtonVariant) {
  return {
    [variant]: () => <Button variant={variant}>Continue</Button>,
    [`${variant}, disabled`]: () => (
      <Button variant={variant} disabled>
        Continue
      </Button>
    ),
    [`${variant}, busy`]: () => (
      <Button variant={variant} loading>
        Continue
      </Button>
    ),
  };
}

/** Radios share a group per drawing, so the light and dark galleries never take each other's choice. */
function Radios() {
  const group = useId();
  return (
    <div className="button-specimen">
      <CheckRow
        type="radio"
        name={group}
        label="Radio"
        checked={false}
        onChange={none}
      />
      <CheckRow
        type="radio"
        name={group}
        label="Chosen"
        checked
        onChange={none}
      />
    </div>
  );
}

/** A field around its own control; ids per drawing, since the gallery draws every state in both themes. */
function KeyField({ error }: { error?: string }) {
  const id = useId();
  return (
    <Field id={id} label="Shared key" error={error}>
      <input
        id={id}
        className="form-control"
        defaultValue={error ? "search key" : "SEARCH_API_KEY"}
        aria-invalid={error ? true : undefined}
      />
    </Field>
  );
}

/** A name edited in place opens from Rename, as in the app: editing takes the focus, which the page must keep. */
function Rename() {
  const [name, setName] = useState("Quarterly review");
  const [editing, setEditing] = useState(false);
  return editing ? (
    <InlineEdit
      value={name}
      label="Workspace name"
      onCommit={(value) => {
        setName(value);
        setEditing(false);
        return true;
      }}
      onCancel={() => setEditing(false)}
    />
  ) : (
    <Button variant="ghost" onClick={() => setEditing(true)}>
      {name}: Rename
    </Button>
  );
}

/** A picker that keeps its choice, so the open list shows it checked. */
function Picker({ disabled = false }: { disabled?: boolean }) {
  const [value, setValue] = useState("small");
  return (
    <MenuButton
      label="Environment size"
      options={[
        { id: "small", label: "Small", detail: "2 CPUs, 4 GB" },
        { id: "large", label: "Large", detail: "8 CPUs, 16 GB" },
        {
          id: "gpu",
          label: "GPU",
          detail: "Not on this installation",
          disabled: "No GPU here",
        },
      ]}
      value={value}
      onChange={setValue}
      disabled={disabled}
      disabledReason={
        disabled ? "Only an administrator changes this" : undefined
      }
    />
  );
}

/** Tabs that switch, with the chosen view's panel, as the channel's side panel has them. */
function Tabs() {
  const [current, setCurrent] = useState<"files" | "preview" | "notes">(
    "files",
  );
  const id = useId();
  return (
    <div className="stack">
      <TabList
        id={id}
        label="Panel"
        views={[
          { value: "files", label: "Files" },
          { value: "preview", label: "Preview" },
          { value: "notes", label: "Notes" },
        ]}
        current={current}
        onChoose={setCurrent}
      />
      <div role="tabpanel" {...tabPanelIds(id, current)}>
        <p className="type-small muted">The {current} view.</p>
      </div>
    </div>
  );
}

/** Buttons, icon buttons, menus, tabs and form fields (UI-GUIDELINES §7). */
export const CONTROLS: GallerySection = {
  Button: {
    ...buttons("primary"),
    ...buttons("secondary"),
    ...buttons("ghost"),
    ...buttons("danger"),
    "long label": () => <Button variant="secondary">{LONG}</Button>,
  },
  IconAction: {
    ghost: () => (
      <IconAction
        glyph={Plus}
        name="Attach files"
        tip="Attach files"
        onClick={none}
      />
    ),
    "round, send": () => (
      <IconAction
        glyph={ArrowUp}
        round
        name="Send"
        tip="Send"
        variant="primary"
        fill="ink"
        onClick={none}
      />
    ),
    "round, send, empty": () => (
      <IconAction
        glyph={ArrowUp}
        round
        name="Send"
        tip="Send"
        variant="primary"
        fill="outline"
        disabled
        onClick={none}
      />
    ),
    "round, sending": () => (
      <IconAction
        glyph={ArrowUp}
        round
        name="Sending"
        tip="Sending"
        variant="primary"
        fill="ink"
        loading
        onClick={none}
      />
    ),
  },
  Icon: {
    "16 px": () => <Icon glyph={Search} />,
    "20 px": () => <Icon glyph={Search} size={20} />,
  },
  Input: {
    empty: () => (
      <Input label="Workspace name" placeholder="Quarterly review" />
    ),
    filled: () => (
      <Input label="Workspace name" defaultValue="Quarterly review" />
    ),
    disabled: () => (
      <Input label="Workspace name" defaultValue="Read only" disabled />
    ),
    error: () => (
      <Input label="Workspace name" error="Enter a workspace name." />
    ),
    "long value": () => <Input label="Workspace name" defaultValue={LONG} />,
  },
  Textarea: {
    empty: () => (
      <Textarea label="Description" placeholder="Add a description" />
    ),
    disabled: () => (
      <Textarea label="Description" defaultValue="Read only" disabled />
    ),
    error: () => <Textarea label="Description" error="Add a description." />,
  },
  Select: {
    default: () => (
      <Select label="Workspace">
        <option>Default workspace</option>
        <option>Quarterly review</option>
      </Select>
    ),
    disabled: () => (
      <Select label="Workspace" disabled>
        <option>Default workspace</option>
      </Select>
    ),
    error: () => (
      <Select label="Workspace" error="Choose a workspace.">
        <option>Default workspace</option>
      </Select>
    ),
  },
  Field: {
    "with a control": () => <KeyField />,
    error: () => <KeyField error="Use capital letters, digits and _." />,
  },
  InlineEdit: {
    "Rename opens it": () => <Rename />,
  },
  MenuButton: {
    closed: () => <Picker />,
    disabled: () => <Picker disabled />,
  },
  ActionMenu: {
    "the ⋯ trigger": () => (
      <ActionMenu
        label="More actions for Quarterly review"
        actions={[
          { id: "rename", label: "Rename", icon: Pencil, onSelect: none },
          {
            id: "delete",
            label: "Delete",
            icon: Trash2,
            onSelect: none,
            danger: true,
          },
          {
            id: "share",
            label: "Share",
            icon: Search,
            unavailable: "Only its owner can share it",
            onSelect: none,
          },
        ]}
      />
    ),
  },
  TabList: {
    "three views": () => <Tabs />,
  },
  CheckRow: {
    unchecked: () => (
      <CheckRow label="Keep the files" checked={false} onChange={none} />
    ),
    checked: () => <CheckRow label="Keep the files" checked onChange={none} />,
    mixed: () => (
      <CheckRow label="Every workspace" checked={false} mixed onChange={none} />
    ),
    disabled: () => (
      <CheckRow
        label="Keep the files"
        checked={false}
        disabled
        onChange={none}
      />
    ),
    radios: () => <Radios />,
    "long label": () => (
      <CheckRow label={LONG} checked={false} onChange={none} />
    ),
  },
};
