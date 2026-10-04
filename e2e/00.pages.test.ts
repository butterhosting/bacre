import { expect, test } from "@playwright/test";
import { AppBoundary } from "./boundaries/AppBoundary";

test.beforeEach(async ({ page }) => {
  await AppBoundary.seed(page);
});

test("every page loads and has its title", async ({ page }) => {
  // given
  type TestCase = {
    url: string;
    expectation: { title: string; heading: string };
  };
  const testCases: TestCase[] = [
    { url: "", expectation: { title: "Services · Bacre", heading: "8 Services" } },
    { url: "services/wiki", expectation: { title: "wiki · Bacre", heading: "wiki" } },
    { url: "jobs", expectation: { title: "Jobs · Bacre", heading: "Jobs" } },
  ];
  for (const { url, expectation } of testCases) {
    // when
    await page.goto(url);
    // then
    await expect(page).toHaveTitle(expectation.title);
    await expect(page.getByRole("heading", { level: 1 })).toHaveText(expectation.heading);
  }
});

test("an unknown page leads back to the overview", async ({ page }) => {
  // when
  await page.goto("nowhere/at/all");
  // then
  await expect(page).toHaveURL(/\/$/);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("8 Services");
});

test("the overview shows every service in its state", async ({ page }) => {
  // when
  await page.goto("");
  // then
  const row = (service: string) => page.getByRole("row", { name: new RegExp(`^${service}\\b`) });
  for (const service of ["dns", "gallery", "ledger", "mailbox", "radio", "recipes", "tracker", "wiki"]) {
    await expect(row(service)).toBeVisible();
  }
  await expect(row("gallery")).toContainText("listing failed");
  await expect(row("ledger")).toContainText("stale");
  await expect(row("recipes")).toContainText("no snapshots");
  await expect(page.getByText("Fatal: wrong password or no key found")).toBeVisible();
});

test("a service page shows its snapshots and what its bacre.yaml says", async ({ page }) => {
  // when
  await page.goto("");
  await page.getByRole("row", { name: /^wiki\b/ }).click();
  // then
  await expect(page).toHaveURL(/\/services\/wiki$/);
  const offsite = page.getByTestId("restic-card");
  await expect(offsite).toContainText("(total: 4)");
  await expect(offsite.getByTestId("snapshot")).toHaveCount(4);

  // when
  await offsite.getByRole("button", { name: "Show details" }).click();
  // then
  await expect(offsite).toContainText("0 3 * * *");
  await expect(offsite).toContainText("demo");
  await expect(offsite).toContainText("lifecycle.backupPrepare");
  await expect(offsite).toContainText("lifecycle.restoreApply");
});

test("the jobs page starts empty after a seed", async ({ page }) => {
  // when
  await page.goto("jobs");
  // then
  await expect(page.getByText("No jobs since Bacre was last restarted")).toBeVisible();
});

test("the sandbox can be purged and seeded from the sidebar", async ({ page }) => {
  // given
  await page.goto("");
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("8 Services");

  // when
  await page.getByRole("button", { name: "Purge", exact: true }).click();
  // then
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("0 Services");
  await expect(page.getByText("No bacre.yaml found in the atlas.")).toBeVisible();

  // when
  await page.getByRole("button", { name: "Seed", exact: true }).click();
  // then
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("8 Services");
});

test("the page says in the console which build it runs against", async ({ page }) => {
  // given
  const messages: string[] = [];
  page.on("console", (message) => messages.push(message.text()));
  // when
  await page.goto("");
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("8 Services");
  // then
  const build = messages.find((message) => message.includes("Bacre"));
  expect(build).toContain("e2e");
  expect(build).toMatch(/Version\s+\S+/);
  expect(build).toMatch(/Commit\s+[0-9a-f]{7}/);
});
