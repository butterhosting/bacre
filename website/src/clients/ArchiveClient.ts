import { Archives } from "@/models/Archives";
import type { Yesttp } from "yesttp";

export class ArchiveClient {
  public constructor(private readonly yesttp: Yesttp) {}

  public async get(): Promise<Archives.Type> {
    const { json } = await this.yesttp.get<unknown>("/archives");
    return Archives.parse(json);
  }

  public async discard(staged: Pick<Archives.Staged, "service" | "handle">): Promise<void> {
    await this.yesttp.delete(`/staged/${encodeURIComponent(staged.service)}/${encodeURIComponent(staged.handle)}`, { responseType: "text" });
  }
}
