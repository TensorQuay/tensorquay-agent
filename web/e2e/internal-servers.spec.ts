import { signIn } from "./session";
import { axe } from "./settings-page";
import { expect, resetFront, test } from "./test";

// An internal server's rule in Internet access (ADR-0035's amendment, 1 Oct 2026): one exact host name may reach an
// address on the installation's own network; never a wildcard or an address, and a rule the gateway can't use yet
// says why.
test("WEB-U-188 a rule for an internal server, and none for a wildcard or an address", async ({
  page,
}) => {
  await signIn(page);
  await page.goto("/settings/internet-access");
  await page.getByRole("button", { name: "Add a site" }).click();
  const add = page.getByRole("dialog", { name: "Add a rule" });
  const internal = add.getByRole("checkbox", { name: "Internal server" });
  for (const host of ["*.corp.example", "10.1.2.3"]) {
    await add.getByLabel("Host").fill(host);
    await expect(internal).toBeDisabled();
    await expect(
      add.getByText(
        "Only for one exact host name. A rule for an address already reaches it.",
      ),
    ).toBeVisible();
  }
  await add.getByLabel("Host").fill("git.corp.example");
  await add.getByLabel("Ports").fill("443");
  await internal.check();
  await axe(page);
  await add.getByRole("button", { name: "Save rule" }).click();
  await expect(page.getByText("Saved rule git.corp.example.")).toBeVisible();
  await expect(
    page.getByRole("row", { name: /git\.corp\.example/ }),
  ).toContainText("Internal server");
});

test("WEB-U-188 an internal rule the gateway can't use yet says why", async ({
  page,
  request,
}) => {
  await resetFront(request, ["internal-rules"]);
  await signIn(page);
  await page.goto("/settings/internet-access");
  await expect(
    page.getByRole("row", { name: /git\.corp\.example/ }),
  ).toContainText(
    "Not in use yet: name this machine's own addresses in gateway.toml ([egress] internal_never).",
  );
  await expect(
    page.getByRole("row", { name: /wiki\.corp\.example/ }),
  ).toContainText(
    "Not in use for now: the gateway can't read the container engine's networks. It tries again every minute.",
  );
  await axe(page);
});
