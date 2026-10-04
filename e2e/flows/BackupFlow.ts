import { expect, type Page } from "@playwright/test";

export namespace BackupFlow {
  type Start = {
    backend: "btrfs" | "restic";
    services: string[];
  };
  export async function start(page: Page, { backend, services }: Start): Promise<void> {
    await page.goto("");
    await page.getByRole("button", { name: `Backup ${backend}`, exact: true }).click();

    const dialog = page.getByRole("dialog", { name: `Backup ${backend}` });
    await expect(dialog).toBeVisible();
    // every service is ticked when the dialog opens
    await dialog.getByRole("checkbox", { name: /^All services: / }).first().uncheck();
    for (const service of services) {
      await dialog.getByRole("checkbox", { name: `${service}: ${backend === "btrfs" ? "hot" : "backup"}`, exact: true }).check();
    }
    await dialog.getByRole("button", { name: `Backup ${services.length} ${services.length === 1 ? "service" : "services"}`, exact: true }).click();
  }
}
