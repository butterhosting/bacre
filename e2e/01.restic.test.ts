import { expect, test } from "@playwright/test";
import { AppBoundary } from "./boundaries/AppBoundary";
import { SandboxBoundary } from "./boundaries/SandboxBoundary";
import { BackupFlow } from "./flows/BackupFlow";
import { JobFlow } from "./flows/JobFlow";
import { RestoreFlow } from "./flows/RestoreFlow";

test.beforeEach(async ({ page }) => {
  await AppBoundary.seed(page);
});

test("a backup runs the service's hooks and adds a snapshot", async ({ page }) => {
  // when
  await BackupFlow.start(page, { backend: "restic", services: ["wiki"] });
  const { status, log } = await JobFlow.ended(page);

  // then
  expect(status).toBe("succeeded");
  expect(log).toContain("==> Preparing wiki");
  expect(log).toContain("==> Backing up wiki (2 paths)");
  expect(log).toContain("==> Applying retention for wiki");
  expect(log).toContain("==> Releasing wiki");
  expect(await SandboxBoundary.state("wiki")).toBe("running");
  expect(await SandboxBoundary.dumps("wiki")).toEqual([]);

  // when
  await page.goto("services/wiki");
  // then
  await expect(page.getByTestId("restic-card")).toContainText("(total: 5)");
});

test("the first backup of a service creates its repository", async ({ page }) => {
  // given
  await page.goto("");
  const recipes = page.getByRole("row", { name: /^recipes\b/ });
  await expect(recipes).toContainText("no snapshots");

  // when
  await BackupFlow.start(page, { backend: "restic", services: ["recipes"] });
  const { status, log } = await JobFlow.ended(page);

  // then
  expect(status).toBe("succeeded");
  expect(log).toContain("No repository yet for recipes, initialising it");
  await page.goto("");
  await expect(recipes).not.toContainText("no snapshots");
});

test("one service failing does not cost the others their backup", async ({ page }) => {
  // when
  await BackupFlow.start(page, { backend: "restic", services: ["wiki", "gallery"] });
  const { status, log } = await JobFlow.ended(page);

  // then
  expect(status).toBe("failed");
  expect(log).toContain("1 of 2 failed: gallery");
  expect(log).toContain("wrong password or no key found");
  await page.goto("services/wiki");
  await expect(page.getByTestId("restic-card")).toContainText("(total: 5)");
});

test("a download and a restore bring back what was lost", async ({ page }) => {
  // given
  const readme = await SandboxBoundary.read("wiki", "data/README.md");
  const database = await SandboxBoundary.read("wiki", "db/wiki.db");
  await SandboxBoundary.write("wiki", "data/README.md", "OVERWRITTEN\n");
  await SandboxBoundary.write("wiki", "data/stray.txt", "should not survive the restore\n");
  await SandboxBoundary.write("wiki", "db/wiki.db", "CORRUPT\n");

  // when
  await RestoreFlow.download(page, "wiki");
  // then
  expect((await JobFlow.ended(page)).status).toBe("succeeded");
  expect(await SandboxBoundary.staged("wiki")).toHaveLength(1);
  expect(await SandboxBoundary.read("wiki", "data/README.md")).toBe("OVERWRITTEN\n");
  await page.goto("");
  await expect(page.getByRole("row", { name: /^wiki\b/ })).toContainText("staged");

  // when
  await RestoreFlow.restoreStaged(page, "wiki");
  // then
  expect((await JobFlow.ended(page)).status).toBe("succeeded");
  expect(await SandboxBoundary.read("wiki", "data/README.md")).toBe(readme);
  expect(await SandboxBoundary.read("wiki", "db/wiki.db")).toBe(database);
  expect(await SandboxBoundary.exists("wiki", "data/stray.txt")).toBe(false);
  expect(await SandboxBoundary.state("wiki")).toBe("running");

  // when
  await RestoreFlow.discard(page, "wiki");
  // then
  expect(await SandboxBoundary.staged("wiki")).toEqual([]);
  await page.goto("");
  await expect(page.getByRole("row", { name: /^wiki\b/ })).not.toContainText("staged");
});

test("a service without a restore hook can be downloaded but not restored", async ({ page }) => {
  // given
  await RestoreFlow.download(page, "tracker");
  expect((await JobFlow.ended(page)).status).toBe("succeeded");

  // when
  await page.goto("services/tracker");
  await page.getByTestId("staged").getByRole("button", { name: "Restore", exact: true }).click();
  // then
  const dialog = page.getByRole("dialog", { name: "Restore tracker" });
  await expect(dialog).toContainText("declares no lifecycle.restoreApply hook");

  // when
  await dialog.getByRole("textbox").fill("tracker");
  await dialog.getByRole("button", { name: "Restore", exact: true }).click();
  const { status, log } = await JobFlow.ended(page);
  // then
  expect(status).toBe("failed");
  expect(log).toContain("tracker has no restic.lifecycle.restoreApply hook");
});
