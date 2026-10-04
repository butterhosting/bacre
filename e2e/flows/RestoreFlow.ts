import { expect, type Page } from "@playwright/test";

export namespace RestoreFlow {
  export async function download(page: Page, service: string): Promise<void> {
    await page.goto(`services/${service}`);
    const card = page.getByTestId("restic-card");
    await card.getByTestId("snapshot").first().getByRole("button", { name: "Download", exact: true }).click();
  }

  export async function restoreSnapshot(page: Page, service: string, handle: string): Promise<void> {
    await page.goto(`services/${service}`);
    const row = page.getByTestId("btrfs-card").getByTestId("snapshot").filter({ hasText: handle });
    await row.getByRole("button", { name: "Restore", exact: true }).click();
    await confirm(page, service);
  }

  export async function restoreStaged(page: Page, service: string): Promise<void> {
    await page.goto(`services/${service}`);
    await page.getByTestId("staged").getByRole("button", { name: "Restore", exact: true }).click();
    await confirm(page, service);
  }

  async function confirm(page: Page, service: string): Promise<void> {
    const dialog = page.getByRole("dialog", { name: `Restore ${service}` });
    await expect(dialog).toBeVisible();
    const confirm = dialog.getByRole("button", { name: "Restore", exact: true });
    await expect(confirm).toBeDisabled();
    await dialog.getByRole("textbox").fill(service);
    await confirm.click();
  }

  export async function discard(page: Page, service: string): Promise<void> {
    await page.goto(`services/${service}`);
    await page.getByTestId("staged").getByRole("button", { name: "Discard", exact: true }).click();
    await page.getByRole("dialog", { name: `Discard ${service}` }).getByRole("button", { name: "Discard", exact: true }).click();
    await expect(page.getByTestId("staged")).not.toBeVisible();
  }
}
