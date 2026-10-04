import type { Archives } from "@/models/Archives";
import type { Jobs } from "@/models/Jobs";
import { useState, type ReactNode } from "react";
import { useNavigate } from "react-router";
import { JobClient } from "../clients/JobClient";
import { Prettify } from "../helpers/Prettify";
import { useRegistry } from "../hooks/useRegistry";
import { Route } from "../Route";
import { Button } from "./Button";
import { Chip } from "./Chip";

type Props = {
  service: Archives.Service;
  /** What is being put back */
  snapshot: Archives.Snapshot;
  /** Where the download sits, for the backends that stage one first */
  staged?: Archives.Staged;
  onClose: () => void;
};
/**
 * The last stop before a service's live data is replaced: what goes back, what will run,
 * how recent the newest local snapshot is, and the service's name typed out to confirm.
 */
export function RestoreDialog({ service, snapshot, staged, onClose }: Props) {
  const jobClient = useRegistry(JobClient);
  const navigate = useNavigate();
  const [typed, setTyped] = useState("");
  const [error, setError] = useState<ReactNode>();
  const [starting, setStarting] = useState(false);

  const latestLocal = service.snapshots.find((s) => s.backend === "btrfs");
  const info = service.backends[snapshot.backend]?.info;

  async function start() {
    setStarting(true);
    setError(undefined);
    try {
      const request: Jobs.RestoreRequest = { kind: "restore", backend: snapshot.backend, service: service.name, handle: snapshot.handle };
      const job = await jobClient.start(request);
      void navigate(Route.job(job.id));
    } catch (e) {
      if (e instanceof JobClient.Busy) {
        setError(
          <>
            Another job is still running: <a href={Route.job(e.jobId)}>open it</a>.
          </>,
        );
      } else {
        setError(e instanceof Error ? e.message : String(e));
      }
      setStarting(false);
    }
  }

  return (
    <div className="fixed inset-0 z-10 flex items-center justify-center bg-[#6b6259]/80 p-3 md:p-6" onClick={onClose}>
      <div className="max-h-full w-[600px] max-w-full overflow-y-auto rounded-[14px] border border-c-line bg-c-card" onClick={(e) => e.stopPropagation()}>
        <div className="flex flex-col gap-3.5 px-6 pt-5 pb-4.5">
          <h2 className="font-display text-2xl font-semibold">Restore {service.name}</h2>

          <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-sm">
            <dt className="text-c-muted">from</dt>
            <dd className="flex flex-wrap items-center gap-2">
              <Chip backend={snapshot.backend} />
              <code className="font-mono text-xs">{snapshot.handle}</code>
              <span className="text-c-muted">· {Prettify.relativeDay(snapshot.time)}</span>
            </dd>
            {staged && (
              <>
                <dt className="text-c-muted">staged at</dt>
                <dd>
                  <code className="font-mono text-xs break-all">{staged.path}</code>
                </dd>
              </>
            )}
            <dt className="text-c-muted">runs</dt>
            <dd>{info ? <Internal.Runs info={info} /> : null}</dd>
          </dl>

          <div className="rounded-lg border border-c-line bg-c-cardhead px-3 py-2.5 text-sm text-c-ink2">
            {latestLocal ? (
              <>
                The newest local snapshot of {service.name} is from <strong className="font-semibold text-c-ink">{Prettify.relativeDay(latestLocal.time)}</strong>. Anything newer
                than that is gone after this restore, unless you back up first.
              </>
            ) : (
              <>There is no local snapshot of {service.name} to fall back on. Whatever is live now is gone after this restore, unless you back up first.</>
            )}
          </div>

          <label className="flex flex-col gap-1.5 text-sm">
            <span>
              Type <code className="font-mono font-semibold">{service.name}</code> to confirm
            </span>
            <input
              type="text"
              value={typed}
              onChange={(e) => setTyped(e.target.value)}
              placeholder={service.name}
              autoFocus
              className="rounded-lg border border-[#d8cbb9] bg-c-card px-3 py-2 font-mono text-sm outline-none focus:border-c-ink"
            />
          </label>
        </div>

        <div className="flex items-center justify-between gap-4 border-t border-c-line bg-c-cardhead px-6 pt-3.5 pb-4.5">
          <button type="button" onClick={onClose} className="cursor-pointer text-sm font-medium text-c-ink2 hover:text-c-ink">
            Cancel
          </button>
          {error && <span className="text-sm text-c-warn">{error}</span>}
          <Button primary onClick={() => void start()} loading={starting} disabled={typed !== service.name}>
            Restore
          </Button>
        </div>
      </div>
    </div>
  );
}

namespace Internal {
  const HOOK = "rounded bg-c-cardhead px-1.5 py-0.5 font-mono text-xs";

  /** The hooks a restore runs, by their bacre.yaml path; a new backend does not compile without a case */
  export function Runs({ info }: { info: Archives.BackendInfo }) {
    switch (info.backend) {
      case "btrfs":
        return info.lifecycle ? (
          <span className="flex flex-wrap items-center gap-1.5">
            <code className={HOOK}>lifecycle.stop</code>
            <span className="text-c-muted">→ swap the subvolume →</span>
            <code className={HOOK}>lifecycle.start</code>
          </span>
        ) : (
          <span className="text-c-warn">nothing: this service declares no lifecycle hooks, so the restore will refuse to run</span>
        );
      case "restic":
        return info.hooks.restore ? <code className={HOOK}>lifecycle.restoreApply</code> : <span className="text-c-warn">nothing: this service declares no lifecycle.restoreApply hook, so the restore will refuse to run</span>;
      default:
        return info satisfies never;
    }
  }
}
