"use client";

import { ArrowUp, Plus, Search } from "lucide-react";
import { useId } from "react";

import { Button, type ButtonVariant } from "../ui/Button";
import { CheckRow } from "../ui/CheckRow";
import { Icon } from "../ui/Icon";
import { IconAction } from "../ui/IconAction";
import { Input } from "../ui/Input";
import { Select } from "../ui/Select";
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

/** Buttons, icon buttons and form fields (UI-GUIDELINES §7). */
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
