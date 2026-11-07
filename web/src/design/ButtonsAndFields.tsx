import { Button, type ButtonVariant } from "../ui/Button";
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

/** Shows button variants and field interaction states using the shared primitives. */
export function ButtonsAndFields() {
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
        <h3 className="type-heading">Form controls</h3>
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
