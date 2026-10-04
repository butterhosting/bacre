import { z } from "zod/v4";

/** What the server says about itself: which build it is, and how it runs */
export namespace Env {
  const Schema = z.object({
    stage: z.enum(["dev", "prod"]),
    /** The tag the build is on, `<tag>-snapshot` past one, `0.0.0` without any */
    version: z.string(),
    /** The short commit hash */
    commit: z.string(),
  });
  export type Type = z.infer<typeof Schema>;

  export function parse(json: unknown): Type {
    return Schema.parse(json);
  }
}
