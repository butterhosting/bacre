import { z } from "zod/v4";

/**
 * The archives are the history: what the server reads out of the backends and what the
 * website renders. This is the website's reading of what the server sends (the server's
 * own definition is in src/models/archives.rs), so timestamps arrive as ISO strings.
 *
 * Every backend reduces to the same four facts about a snapshot: which service, when,
 * which backend, and an opaque handle only that backend can act on. Anything else a
 * backend knows goes in `details`, typed per backend; what it knows about the service
 * as a whole goes in `BackendInfo`, likewise.
 *
 * Adding a backend starts here on the website side: extend `Backend`, and the compiler
 * walks you through the variants below and the website copy.
 */
export namespace Archives {
  export const Backend = z.enum(["btrfs", "restic"]);
  export type Backend = z.infer<typeof Backend>;
  export const BACKENDS: readonly Backend[] = Backend.options;

  const Common = {
    service: z.string(),
    time: z.iso.datetime({ offset: true }),
    /** Backend-specific identifier: the subvolume name, the restic snapshot id, … */
    handle: z.string(),
  };
  export const Snapshot = z.discriminatedUnion("backend", [
    z.object({
      backend: z.literal("btrfs"),
      ...Common,
      details: z.object({
        /** How many of the service's targets also hold this snapshot */
        onTargets: z.number().int().nonnegative(),
        targets: z.number().int().nonnegative(),
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

  /** What a backend can say about a service beyond its snapshots: its configuration, and what it found */
  export const BackendInfo = z.discriminatedUnion("backend", [
    z.object({
      backend: z.literal("btrfs"),
      /** Absolute path of the live subvolume */
      subvolume: z.string(),
      /** Where its snapshots go */
      snapshots: z.string(),
      targets: z.array(z.string()),
      retention: z.object({ preserveMin: z.string(), preserve: z.array(z.string()) }),
      /** The stop and start hooks, when the service can be taken lifecycle (cold snapshots, restores) */
      lifecycle: z.object({ stop: z.string(), start: z.string() }).nullable(),
    }),
    z.object({
      backend: z.literal("restic"),
      repository: z.string(),
      envset: z.string(),
      retention: z.object({ keepLast: z.number(), keepDaily: z.number(), keepWeekly: z.number(), keepMonthly: z.number() }),
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

  /** What a backend had to say about one service on the last refresh */
  export const BackendStatus = z.object({
    /** `absent`: the backend has nothing for this service yet (no repository, no snapshots) */
    state: z.enum(["ok", "absent", "error"]),
    message: z.string().optional(),
    info: BackendInfo,
  });
  export type BackendStatus = z.infer<typeof BackendStatus>;

  export const Service = z.object({
    name: z.string(),
    /** Only the backends its bacre.yaml configures */
    backends: z.partialRecord(Backend, BackendStatus),
    /** All backends together, newest first */
    snapshots: z.array(Snapshot),
  });
  export type Service = z.infer<typeof Service>;

  /**
   * Something the last refresh could not do; the archives themselves are untouched.
   * Deliberately untyped beyond "where" and "what": any error anywhere fits.
   */
  export const Problem = z.object({
    /** Where it came from: a file, a backend, a backend and service, … */
    source: z.string(),
    message: z.string(),
  });
  export type Problem = z.infer<typeof Problem>;

  /** A downloaded snapshot waiting in the staging directory, to be inspected, restored or discarded */
  export const Staged = z.object({
    service: z.string(),
    backend: Backend,
    handle: z.string(),
    path: z.string(),
    downloadedAt: z.iso.datetime({ offset: true }),
  });
  export type Staged = z.infer<typeof Staged>;

  /** A backend of a service that Bacre backs up by itself */
  export const Schedule = z.object({
    service: z.string(),
    backend: Backend,
    cron: z.string(),
    /** Null for an expression that never fires */
    next: z.iso.datetime({ offset: true }).nullable(),
    /** Due already, and waiting for the running job to finish */
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
