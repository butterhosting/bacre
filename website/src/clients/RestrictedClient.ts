import type { Yesttp } from "yesttp";

export class RestrictedClient {
  public constructor(private readonly yesttp: Yesttp) {}

  public async seed(): Promise<void> {
    await this.yesttp.post("/restricted/seed", { responseType: "text" });
  }

  public async purge(): Promise<void> {
    await this.yesttp.post("/restricted/purge", { responseType: "text" });
  }
}
