"use client";

import { GALLERY } from "./gallery";

/**
 * The component gallery in one theme (WEB-U-510): each component in web/src/ui with every state its registry entry
 * names, each state captioned. Drawn below the token reference, so that panel's own screenshot stays as it is.
 */
export function GalleryPanel({ theme }: { theme: "light" | "dark" }) {
  return (
    <section
      className="design-panel"
      data-theme={theme}
      data-testid={`gallery-${theme}`}
      aria-label={`${theme} components`}
    >
      <header>
        <h2 className="type-heading">
          {theme === "light" ? "Components, light" : "Components, dark"}
        </h2>
      </header>
      {Object.entries(GALLERY).map(([name, states]) => (
        <section
          className="design-section"
          key={name}
          data-gallery-component={name}
        >
          <h3 className="type-heading">{name}</h3>
          <div className="gallery-states">
            {Object.entries(states).map(([state, draw]) => (
              <figure
                className="gallery-state"
                key={state}
                data-gallery-state={state}
              >
                {draw()}
                <figcaption className="type-label muted">{state}</figcaption>
              </figure>
            ))}
          </div>
        </section>
      ))}
    </section>
  );
}
