import { expect, test } from "@playwright/test";
import { AppBoundary } from "./boundaries/AppBoundary";
import { SandboxBoundary } from "./boundaries/SandboxBoundary";
import { BackupFlow } from "./flows/BackupFlow";
import { JobFlow } from "./flows/JobFlow";
import { RestoreFlow } from "./flows/RestoreFlow";

test.beforeEach(async ({ page }) => {
  await AppBoundary.seed(page);
});

test("a cold backup stops the service only for the snapshot, then sends just the changes", async ({ page }) => {
  // given
  const before = await SandboxBoundary.snapshots("disk-a", "wiki");

  // when
  await BackupFlow.start(page, { backend: "btrfs", services: ["wiki"], mode: "cold" });
  const { status, log } = await JobFlow.ended(page);

  // then
  expect(status).toBe("succeeded");
  const steps = ["==> Stopping wiki", "==> Snapshotting", "==> Starting wiki", `as the changes since ${before.at(-1)}`];
  const positions = steps.map((step) => log.indexOf(step));
  expect(positions.every((position) => position >= 0), `all of ${steps.join(", ")} in:\n${log}`).toBe(true);
  expect(positions).toEqual([...positions].sort((a, b) => a - b));
  const after = await SandboxBoundary.snapshots("disk-a", "wiki");
  expect(after).toHaveLength(before.length + 1);
  expect(await SandboxBoundary.snapshots("disk-b", "wiki")).toEqual(after);
  expect(await SandboxBoundary.state("wiki")).toBe("running");

  // when
  await page.goto("services/wiki");
  // then
  await expect(page.getByTestId("btrfs-card")).toContainText(`(total: ${after.length})`);
});

test("a snapshot that never reached a disk is shown, and the next one builds on what both disks have", async ({ page }) => {
  // given
  const unsent = (await SandboxBoundary.snapshots("disk-a", "dns")).at(-1)!;
  const shared = (await SandboxBoundary.snapshots("disk-b", "dns")).at(-1)!;
  expect(shared).not.toBe(unsent);
  await page.goto("services/dns");
  const row = page.getByTestId("btrfs-card").getByTestId("snapshot").filter({ hasText: unsent });
  await expect(row).toContainText("missing in 1 of 2 snapshot paths");

  // when
  await BackupFlow.start(page, { backend: "btrfs", services: ["dns"] });
  const { status, log } = await JobFlow.ended(page);

  // then
  expect(status).toBe("succeeded");
  expect(log).toContain(`as the changes since ${shared}`);
  // dns keeps its last 2, on each disk by itself
  const newest = (await SandboxBoundary.snapshots("disk-a", "dns")).at(-1)!;
  expect(await SandboxBoundary.snapshots("disk-a", "dns")).toEqual([unsent, newest]);
  expect(await SandboxBoundary.snapshots("disk-b", "dns")).toEqual([shared, newest]);
});

test("a restore swaps the live subvolume for a snapshot and brings back what was lost", async ({ page }) => {
  // given
  const readme = await SandboxBoundary.read("wiki", "data/README.md");
  const database = await SandboxBoundary.read("wiki", "db/wiki.db");
  await SandboxBoundary.write("wiki", "data/README.md", "OVERWRITTEN\n");
  await SandboxBoundary.write("wiki", "data/stray.txt", "should not survive the restore\n");
  await SandboxBoundary.write("wiki", "db/wiki.db", "CORRUPT\n");
  const handle = (await SandboxBoundary.snapshots("disk-a", "wiki")).at(-1)!;

  // when
  await RestoreFlow.restoreSnapshot(page, "wiki", handle);
  const { status, log } = await JobFlow.ended(page);

  // then
  expect(status).toBe("succeeded");
  expect(log).toContain(`==> Restoring ${handle} into`);
  expect(await SandboxBoundary.read("wiki", "data/README.md")).toBe(readme);
  expect(await SandboxBoundary.read("wiki", "db/wiki.db")).toBe(database);
  expect(await SandboxBoundary.exists("wiki", "data/stray.txt")).toBe(false);
  expect(await SandboxBoundary.state("wiki")).toBe("running");
  expect(await SandboxBoundary.onDisk("disk-a", "@wiki.bacre-replaced")).toBe(false);
});

test("a service without lifecycle hooks cannot be restored", async ({ page }) => {
  // given
  const handle = (await SandboxBoundary.snapshots("disk-a", "radio")).at(-1)!;
  await page.goto("services/radio");

  // when
  await page.getByTestId("btrfs-card").getByTestId("snapshot").filter({ hasText: handle }).getByRole("button", { name: "Restore", exact: true }).click();
  // then
  const dialog = page.getByRole("dialog", { name: "Restore radio" });
  await expect(dialog).toContainText("declares no lifecycle hooks");

  // when
  await dialog.getByRole("textbox").fill("radio");
  await dialog.getByRole("button", { name: "Restore", exact: true }).click();
  const { status, log } = await JobFlow.ended(page);
  // then
  expect(status).toBe("failed");
  expect(log).toContain("radio has no btrfs.lifecycle block");
  expect(await SandboxBoundary.onDisk("disk-a", "@radio")).toBe(true);
});

test("a send that stops half way fails the job but leaves no half copy behind", async ({ page }) => {
  // given
  const replica = await SandboxBoundary.snapshots("disk-b", "wiki");
  await SandboxBoundary.failReceives("disk-b", true);

  // when
  await BackupFlow.start(page, { backend: "btrfs", services: ["wiki"] });
  const { status, log } = await JobFlow.ended(page);

  // then
  expect(status).toBe("failed");
  expect(log).toContain("receive was interrupted");
  expect(log).toContain("==> Deleting what an interrupted send left in");
  expect(await SandboxBoundary.snapshots("disk-b", "wiki")).toEqual(replica);
  const unsent = (await SandboxBoundary.snapshots("disk-a", "wiki")).at(-1)!;
  await page.goto("services/wiki");
  await expect(page.getByTestId("btrfs-card").getByTestId("snapshot").filter({ hasText: unsent })).toContainText("missing in 1 of 2 snapshot paths");
});

test("retention never deletes the snapshot a lagging disk needs to catch up", async ({ page }) => {
  // given
  const shared = (await SandboxBoundary.snapshots("disk-b", "dns")).at(-1)!;
  await SandboxBoundary.failReceives("disk-b", true);
  await BackupFlow.start(page, { backend: "btrfs", services: ["dns"] });
  expect((await JobFlow.ended(page)).status).toBe("failed");

  // then dns keeps its last 2, and the newest one disk-b has as well
  const local = await SandboxBoundary.snapshots("disk-a", "dns");
  expect(local).toHaveLength(3);
  expect(local[0]).toBe(shared);

  // when
  await SandboxBoundary.failReceives("disk-b", false);
  await BackupFlow.start(page, { backend: "btrfs", services: ["dns"] });
  const { status, log } = await JobFlow.ended(page);

  // then
  expect(status).toBe("succeeded");
  expect(log).toContain(`as the changes since ${shared}`);
  const newest = (await SandboxBoundary.snapshots("disk-a", "dns")).at(-1)!;
  expect(await SandboxBoundary.snapshots("disk-a", "dns")).toEqual([local[2], newest]);
  expect(await SandboxBoundary.snapshots("disk-b", "dns")).toEqual([shared, newest]);
});
