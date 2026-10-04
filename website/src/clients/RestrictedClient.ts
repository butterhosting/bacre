import type { Yesttp } from "yesttp";

/** The dev stage's sandbox: never there in production */
export class RestrictedClient {
  public constructor(private readonly yesttp: Yesttp) {}

  /** Empties the sandbox and fills it with the made-up services again; takes a few seconds */
  public async seed(): Promise<void> {
    await this.yesttp.post("/restricted/seed", { responseType: "text" });
  }

  /** Empties the sandbox */
  public async purge(): Promise<void> {
    await this.yesttp.post("/restricted/purge", { responseType: "text" });
  }
}
