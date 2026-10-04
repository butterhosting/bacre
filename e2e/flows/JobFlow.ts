import { expect, type Page } from "@playwright/test";

export namespace JobFlow {
  type Ended = {
    status: "succeeded" | "failed";
    log: string;
  };
  export async function ended(page: Page): Promise<Ended> {
    await expect(page).toHaveURL(/\/jobs\/[0-9a-f]{8}$/);
    const status = page.getByTestId("status");
    await expect(status).toHaveText(/succeeded|failed/);
    await expect(page.getByText(/==> (Done|Failed: )/)).toBeVisible();
    const text = await status.innerText();
    return {
      status: text.includes("succeeded") ? "succeeded" : "failed",
      log: await page.locator("main").innerText(),
    };
  }
}
