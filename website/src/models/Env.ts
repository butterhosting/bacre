import { z } from "zod/v4";

export namespace Env {
  const Schema = z.object({
    stage: z.enum(["dev", "e2e", "prod"]),
    version: z.string(),
    commit: z.string(),
  });
  export type Type = z.infer<typeof Schema>;

  export function parse(json: unknown): Type {
    return Schema.parse(json);
  }
}
