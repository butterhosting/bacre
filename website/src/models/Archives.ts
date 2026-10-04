import { z } from "zod/v4";

/** The website's reading of what the server sends; the server's own definition is in src/models/archives.rs */
export namespace Archives {
  export const Backend = z.enum(["btrfs", "restic"]);
  export type Backend = z.infer<typeof Backend>;
  export const BACKENDS: readonly Backend[] = Backend.options;

  const Common = {
    service: z.string(),
    time: z.iso.datetime({ offset: true }),
    handle: z.string(),
  };
  export const Snapshot = z.discriminatedUnion("backend", [
    z.object({
      backend: z.literal("btrfs"),
      ...Common,
      details: z.object({
        onDestinations: z.number().int().nonnegative(),
        destinations: z.number().int().nonnegative(),
      }),
    }),
    z.object({
      backend: z.literal("restic"),
      ...Common,
      details: z.object({
        tags: z.array(z.string()),
        paths: z.array(z.string()),
      }),
    }),
  ]);
  export type Snapshot = z.infer<typeof Snapshot>;

  export const Retention = z.object({
    keepLast: z.number(),
    keepHourly: z.number(),
    keepDaily: z.number(),
    keepWeekly: z.number(),
    keepMonthly: z.number(),
  });
  export type Retention = z.infer<typeof Retention>;

  export const BackendInfo = z.discriminatedUnion("backend", [
    z.object({
      backend: z.literal("btrfs"),
      subvolume: z.string(),
      destinations: z.array(z.string()),
      retention: Retention,
      lifecycle: z.object({ stop: z.string(), start: z.string() }).nullable(),
    }),
    z.object({
      backend: z.literal("restic"),
      repository: z.string(),
      envset: z.string(),
      retention: Retention,
      paths: z.array(z.string()),
      hooks: z.object({ prepare: z.string().optional(), release: z.string().optional(), restore: z.string().optional() }),
    }),
  ]);
  export type BackendInfo = z.infer<typeof BackendInfo>;

  // Compile-time checks: every backend has a snapshot variant and an info variant above
  type Covers<T extends { backend: Backend }> = [T["backend"]] extends [Backend] ? ([Backend] extends [T["backend"]] ? true : never) : never;
  const _snapshots: Covers<Snapshot> = true;
  const _infos: Covers<BackendInfo> = true;
  void _snapshots;
  void _infos;

  export const BackendStatus = z.object({
    state: z.enum(["ok", "absent", "error"]),
    message: z.string().optional(),
    info: BackendInfo,
  });
  export type BackendStatus = z.infer<typeof BackendStatus>;

  export const Service = z.object({
    name: z.string(),
    backends: z.partialRecord(Backend, BackendStatus),
    snapshots: z.array(Snapshot),
  });
  export type Service = z.infer<typeof Service>;

  export const Problem = z.object({
    source: z.string(),
    message: z.string(),
  });
  export type Problem = z.infer<typeof Problem>;

  export const Staged = z.object({
    service: z.string(),
    backend: Backend,
    handle: z.string(),
    path: z.string(),
    downloadedAt: z.iso.datetime({ offset: true }),
  });
  export type Staged = z.infer<typeof Staged>;

  export const Schedule = z.object({
    service: z.string(),
    backend: Backend,
    cron: z.string(),
    next: z.iso.datetime({ offset: true }).nullable(),
    waiting: z.boolean(),
  });
  export type Schedule = z.infer<typeof Schedule>;

  const Schema = z.object({
    refreshedAt: z.iso.datetime({ offset: true }).nullable(),
    refreshing: z.boolean(),
    errors: z.array(Problem),
    services: z.array(Service),
    staged: z.array(Staged),
    schedules: z.array(Schedule),
  });
  export type Type = z.infer<typeof Schema>;

  export function parse(json: unknown): Type {
    return Schema.parse(json);
  }
}
