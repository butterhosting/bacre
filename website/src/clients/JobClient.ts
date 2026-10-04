import { Jobs } from "@/models/Jobs";
import type { Yesttp } from "yesttp";

export class JobClient {
  public constructor(private readonly yesttp: Yesttp) {}

  public async list(): Promise<Jobs.Summary[]> {
    const { json } = await this.yesttp.get<unknown[]>("/jobs");
    return json.map((j) => Jobs.Summary.parse(j));
  }

  public async get(id: string): Promise<Jobs.Job> {
    const { json } = await this.yesttp.get<unknown>(`/jobs/${encodeURIComponent(id)}`);
    return Jobs.Job.parse(json);
  }

  public async start(request: Jobs.Request): Promise<Jobs.Job> {
    try {
      const { json } = await this.yesttp.post<unknown>("/jobs", { body: request });
      return Jobs.Job.parse(json);
    } catch (e) {
      const response = (e as { response?: { status?: number; json?: { jobId?: string } } }).response;
      if (response?.status === 409 && response.json?.jobId) {
        throw new JobClient.Busy(response.json.jobId);
      }
      throw e;
    }
  }

  public events(id: string): EventSource {
    return new EventSource(`/api/jobs/${encodeURIComponent(id)}/events`);
  }
}

export namespace JobClient {
  export class Busy extends Error {
    public constructor(public readonly jobId: string) {
      super("Another job is still running");
    }
  }
}
