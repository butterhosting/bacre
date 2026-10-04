import { Env } from "@/models/Env";
import type { Class } from "@/types/Class";
import { createContext } from "react";
import { Yesttp } from "yesttp";
import { ArchiveClient } from "./clients/ArchiveClient";
import { ChangeClient } from "./clients/ChangeClient";
import { JobClient } from "./clients/JobClient";
import { RestrictedClient } from "./clients/RestrictedClient";

export class ClientRegistry {
  private readonly registry: Record<string, any> = {};

  private constructor(
    yesttp: Yesttp,
    public readonly env: Env.Type,
  ) {
    this.registry[Yesttp.name] = yesttp;
    this.registry[ArchiveClient.name] = new ArchiveClient(yesttp);
    this.registry[JobClient.name] = new JobClient(yesttp);
    this.registry[ChangeClient.name] = new ChangeClient();
    this.registry[RestrictedClient.name] = new RestrictedClient(yesttp);
  }

  /** The first request behind the login, so a browser that has to ask for a password asks here */
  public static async bootstrap(): Promise<ClientRegistry> {
    const yesttp = new Yesttp({ baseUrl: "/api" });
    const { json } = await yesttp.get<unknown>("/env");
    const env = Env.parse(json);
    this.printEnv(env);
    return new ClientRegistry(yesttp, env);
  }

  private static printEnv(env: Env.Type) {
    const entries = Object.entries({ Stage: env.stage, Version: env.version, Commit: env.commit });
    const longest = Math.max(...entries.map(([key]) => key.length));
    const lines = entries.map(([key, value]) => `${key.padEnd(longest)}  ${value}`).join("\n");
    console.info("%cBacre\n\n%c%s", "font-size: 24px; font-weight: 800;", "font-size: 12px; font-weight: normal; font-family: monospace;", lines);
  }

  public get<T>(klass: Class<T>): T {
    const result = this.registry[klass.name] as T;
    if (!result) {
      throw new Error(`No registration for ${klass.name}`);
    }
    return result;
  }
}

export namespace ClientRegistry {
  export const Context = createContext({} as ClientRegistry);
}
