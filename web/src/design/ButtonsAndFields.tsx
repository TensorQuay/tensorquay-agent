import { ArrowUp, Plus, Square } from "lucide-react";
import { useId } from "react";

import { Button, type ButtonVariant } from "../ui/Button";
import { CheckRow } from "../ui/CheckRow";
import { IconAction } from "../ui/IconAction";
import { Input } from "../ui/Input";
import { Select } from "../ui/Select";
import { Textarea } from "../ui/Textarea";

const variants: readonly ButtonVariant[] = [
  "primary",
  "secondary",
  "ghost",
  "danger",
];
const states = ["default", "hover", "focus", "disabled", "loading"];
const none = () => undefined;
/** The composer bar's round icon button in each of its states (UI-GUIDELINES §7 Composer). */
const discs = [
  { state: "attach", glyph: Plus, name: "Attach files" },
  { state: "add (New task)", glyph: Plus, name: "Add" },
  {
    state: "send, empty",
    glyph: ArrowUp,
    name: "Send",
    variant: "primary",
    fill: "outline",
    disabled: true,
  },
  {
    state: "send",
    glyph: ArrowUp,
    name: "Send",
    variant: "primary",
    fill: "ink",
  },
  {
    state: "sending",
    glyph: ArrowUp,
    name: "Sending",
    variant: "primary",
    fill: "ink",
    loading: true,
  },
  { state: "stop", glyph: Square, name: "Stop", variant: "secondary" },
] as const;

/** Shows button variants and field interaction states using the shared primitives. */
export function ButtonsAndFields() {
  // Each panel's radios are a group of their own (the light and dark panels show the same specimens).
  const group = useId();
  return (
    <>
      <section className="design-section">
        <h3 className="type-heading">Buttons</h3>
        {variants.map((variant) => (
          <div className="stack" key={variant}>
            <p className="type-label muted">{variant}</p>
            <div className="button-specimen">
              {states.map((state) => (
                <div className="type-sample" key={state}>
                  <Button
                    variant={variant}
                    disabled={state === "disabled"}
                    loading={state === "loading"}
                    data-demo-state={state}
                    data-testid={`${variant}-${state}`}
                  >
                    Continue
                  </Button>
                  <span className="type-small muted">{state}</span>
                </div>
              ))}
            </div>
          </div>
        ))}
      </section>
      <section className="design-section">
        <h3 className="type-heading">Composer icon buttons</h3>
        <div className="button-specimen">
          {discs.map((disc) => (
            <div className="type-sample" key={disc.state}>
              <IconAction
                glyph={disc.glyph}
                round
                name={disc.name}
                tip={disc.name}
                variant={"variant" in disc ? disc.variant : "ghost"}
                fill={"fill" in disc ? disc.fill : undefined}
                disabled={"disabled" in disc ? disc.disabled : undefined}
                loading={"loading" in disc ? disc.loading : false}
                onClick={none}
              />
              <span className="type-small muted">{disc.state}</span>
            </div>
          ))}
        </div>
      </section>
      <section className="design-section">
        <h3 className="type-heading">Form controls</h3>
        <div className="button-specimen">
          <CheckRow label="Checkbox" checked={false} onChange={none} />
          <CheckRow label="Checked" checked onChange={none} />
          <CheckRow label="Disabled" checked={false} disabled onChange={none} />
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
        <div className="specimen-grid">
          <Input label="Input" placeholder="Workspace name" />
          <Input
            label="Focused input"
            data-demo-state="focus"
            placeholder="Workspace name"
          />
          <Input label="Disabled input" defaultValue="Read only" disabled />
          <Input label="Input with error" error="Enter a workspace name." />
          <Textarea label="Textarea" placeholder="Add a description" />
          <Textarea
            label="Focused textarea"
            data-demo-state="focus"
            placeholder="Add a description"
          />
          <Textarea
            label="Disabled textarea"
            defaultValue="Read only"
            disabled
          />
          <Textarea label="Textarea with error" error="Add a description." />
          <Select label="Select">
            <option>Default workspace</option>
            <option>Example workspace</option>
          </Select>
          <Select label="Disabled select" disabled>
            <option>Default workspace</option>
          </Select>
          <Select label="Select with error" error="Choose a workspace.">
            <option value="">Choose workspace</option>
          </Select>
        </div>
      </section>
    </>
  );
}
