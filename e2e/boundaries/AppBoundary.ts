import type { Page } from "@playwright/test";

export namespace AppBoundary {
  export async function purge(page: Page) {
    await page.request.post("/api/restricted/purge", { failOnStatusCode: true });
  }

  export async function seed(page: Page) {
    await page.request.post("/api/restricted/seed", { failOnStatusCode: true, timeout: 30_000 });
  }
}
