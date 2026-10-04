import { z } from "zod/v4";
import type { Archives } from "./Archives";

/**
 * A job is one backup, download or restore running on the server. Jobs live in memory
 * only: the archives are the history, a job is just the act of adding to or reading from them.
 * This is the website's reading of what the server sends and accepts (the server's own
 * definition is in src/models/jobs.rs).
 *
 * Requests are discriminated per backend, so each backend declares exactly the options it
 * has (btrfs: hot or cold; restic: none) and a new backend does not compile without a variant.
 */
export namespace Jobs {
  /** A snapshot's backend-specific identifier; it ends up in paths, hence the narrow alphabet and no leading dot */
  const Handle = z.string().regex(/^[A-Za-z0-9@_-][A-Za-z0-9@._-]*$/);
  const Service = z.string().regex(/^[a-z0-9][a-z0-9_-]*$/);

  export const BackupRequest = z.discriminatedUnion("backend", [
    z.object({
      kind: z.literal("backup"),
      backend: z.literal("btrfs"),
      targets: z
        .array(
          z.object({
            service: Service,
            /** cold stops the service around the snapshot (needs `btrfs.lifecycle` in its bacre.yaml) */
            mode: z.enum(["hot", "cold"]),
          }),
        )
        .min(1),
    }),
    z.object({
      kind: z.literal("backup"),
      backend: z.literal("restic"),
      targets: z.array(z.object({ service: Service })).min(1),
    }),
  ]);
  export type BackupRequest = z.infer<typeof BackupRequest>;

  /**
   * Fetching a snapshot into the staging directory, for the backends whose snapshots are
   * not already on this machine. Restoring is a separate, later request, which leaves
   * room to inspect what was downloaded.
   */
  export const DownloadRequest = z.object({
    kind: z.literal("download"),
    backend: z.literal("restic"),
    service: Service,
    handle: Handle,
  });
  export type DownloadRequest = z.infer<typeof DownloadRequest>;

  /** Putting a snapshot back in place of the live data: btrfs from its snapshot, restic from its staged download */
  export const RestoreRequest = z.discriminatedUnion("backend", [
    z.object({ kind: z.literal("restore"), backend: z.literal("btrfs"), service: Service, handle: Handle }),
    z.object({ kind: z.literal("restore"), backend: z.literal("restic"), service: Service, handle: Handle }),
  ]);
  export type RestoreRequest = z.infer<typeof RestoreRequest>;

  // Compile-time checks: every backend has a backup and a restore request variant
  type Covers<T extends { backend: Archives.Backend }> = [T["backend"]] extends [Archives.Backend] ? ([Archives.Backend] extends [T["backend"]] ? true : never) : never;
  const _backups: Covers<BackupRequest> = true;
  const _restores: Covers<RestoreRequest> = true;
  void _backups;
  void _restores;

  export const Request = z.union([BackupRequest, DownloadRequest, RestoreRequest]);
  export type Request = z.infer<typeof Request>;

  export const Line = z.object({
    at: z.iso.datetime({ offset: true }),
    /** `info` is Bacre narrating, `out`/`err` is what the tools and hooks printed */
    stream: z.enum(["info", "out", "err"]),
    text: z.string(),
  });
  export type Line = z.infer<typeof Line>;

  export const Status = z.enum(["running", "succeeded", "failed"]);
  export type Status = z.infer<typeof Status>;

  /** Who started it: someone on the website, or the scheduler */
  const Trigger = z.enum(["manual", "schedule"]);

  export const Job = z.object({
    id: z.string(),
    trigger: Trigger,
    request: Request,
    title: z.string(),
    status: Status,
    startedAt: z.iso.datetime({ offset: true }),
    endedAt: z.iso.datetime({ offset: true }).nullable(),
    error: z.string().nullable(),
    /** The services known to have failed; a job can fail without knowing (empty) */
    failed: z.array(z.string()),
    lines: z.array(Line),
  });
  export type Job = z.infer<typeof Job>;

  export const Summary = Job.omit({ lines: true });
  export type Summary = z.infer<typeof Summary>;
}
