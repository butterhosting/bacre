import type { Archives } from "@/models/Archives";
import type { Jobs } from "@/models/Jobs";
import { useState, type ReactNode } from "react";
import { useNavigate } from "react-router";
import { JobClient } from "../clients/JobClient";
import { useRegistry } from "../hooks/useRegistry";
import { Route } from "../Route";
import { Button } from "./Button";
import { Chip } from "./Chip";

type Props = {
  backend: Archives.Backend;
  /** The services that can be backed up with this backend */
  candidates: Archives.Service[];
  onClose: () => void;
};
/**
 * A grid: one row per service, one checkbox column per way the backend can back it up.
 * At most one box per row is ticked (none means the service stays out), and the header
 * boxes tick or clear a whole column. The columns come from one map per backend, so a
 * new backend does not compile until it has declared them.
 */
export function BackupDialog({ backend, candidates, onClose }: Props) {
  const jobClient = useRegistry(JobClient);
  const navigate = useNavigate();
  const form = Internal.FORMS[backend];
  const [choice, setChoice] = useState<Internal.Choice>(() => new Map(candidates.map((c) => [c.name, form.columns[0]!.key])));
  const [error, setError] = useState<ReactNode>();
  const [starting, setStarting] = useState(false);

  const chosen = candidates.filter((c) => choice.has(c.name));

  function pick(service: string, column: string, on: boolean) {
    setChoice((current) => {
      const next = new Map(current);
      if (on) {
        next.set(service, column);
      } else {
        next.delete(service);
      }
      return next;
    });
  }

  function pickAll(column: string, on: boolean) {
    setChoice((current) => {
      const next = new Map(current);
      for (const candidate of candidates) {
        if (on) {
          next.set(candidate.name, column);
        } else if (next.get(candidate.name) === column) {
          next.delete(candidate.name);
        }
      }
      return next;
    });
  }

  async function start() {
    setStarting(true);
    setError(undefined);
    try {
      const job = await jobClient.start(form.build(chosen.map((c) => [c.name, choice.get(c.name)!])));
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

  const BOX = "size-4 accent-c-accent";
  const COLUMN = "flex w-16 justify-center";

  return (
    <div className="fixed inset-0 z-10 flex items-center justify-center bg-[#6b6259]/80 p-3 md:p-6" onClick={onClose}>
      <div className="max-h-full w-[600px] max-w-full overflow-y-auto rounded-[14px] border border-c-line bg-c-card" onClick={(e) => e.stopPropagation()}>
        <div className="flex flex-col gap-1.5 px-6 pt-5 pb-3.5">
          <h2 className="flex items-center gap-2.5 font-display text-2xl font-semibold">
            Backup <Chip backend={backend} />
          </h2>
          <span className="text-sm text-c-muted">{form.hint}</span>
        </div>

        <div className="flex items-end border-t border-b border-c-line bg-c-cardhead px-6 py-2 text-xs tracking-wider text-c-muted uppercase">
          <span className="flex-1">Service</span>
          {form.columns.map((column) => (
            <label key={column.key} className={`${COLUMN} cursor-pointer flex-col items-center gap-2`}>
              {column.label && <span>{column.label}</span>}
              <input
                type="checkbox"
                className={BOX}
                checked={candidates.length > 0 && candidates.every((c) => choice.get(c.name) === column.key)}
                onChange={(e) => pickAll(column.key, e.target.checked)}
                aria-label={`All services: ${column.label || "backup"}`}
              />
            </label>
          ))}
        </div>
        {candidates.map((candidate) => (
          <div key={candidate.name} className="flex items-center border-b border-c-line2 px-6 py-2.5">
            <span className="flex-1 font-medium">{candidate.name}</span>
            {form.columns.map((column) => (
              <label key={column.key} className={`${COLUMN} cursor-pointer`}>
                <input
                  type="checkbox"
                  className={BOX}
                  checked={choice.get(candidate.name) === column.key}
                  onChange={(e) => pick(candidate.name, column.key, e.target.checked)}
                  aria-label={`${candidate.name}: ${column.label || "backup"}`}
                />
              </label>
            ))}
          </div>
        ))}
        {candidates.length === 0 && <div className="px-6 py-3 text-sm text-c-muted">No managed service configures this backend.</div>}

        <div className="flex items-center justify-between gap-4 bg-c-cardhead px-6 pt-3.5 pb-4.5">
          <button type="button" onClick={onClose} className="cursor-pointer text-sm font-medium text-c-ink2 hover:text-c-ink">
            Cancel
          </button>
          {error && <span className="text-sm text-c-warn">{error}</span>}
          <Button primary onClick={() => void start()} loading={starting} disabled={chosen.length === 0}>
            Backup {chosen.length} {chosen.length === 1 ? "service" : "services"}
          </Button>
        </div>
      </div>
    </div>
  );
}

namespace Internal {
  /** service → the column it is ticked in */
  export type Choice = Map<string, string>;

  type Form = {
    hint: string;
    /** The ways this backend can back a service up; the first one is ticked when the dialog opens */
    columns: Array<{ key: string; label: string }>;
    build(picked: Array<[service: string, column: string]>): Jobs.BackupRequest;
  };

  export const FORMS: Record<Archives.Backend, Form> = {
    btrfs: {
      hint: "Hot snapshots the service as it runs. Cold stops it first, for a consistent snapshot.",
      columns: [
        { key: "hot", label: "hot" },
        { key: "cold", label: "cold" },
      ],
      build: (picked) => ({ kind: "backup", backend: "btrfs", targets: picked.map(([service, column]) => ({ service, mode: column === "cold" ? "cold" : "hot" })) }),
    },
    restic: {
      hint: "Runs each service's prepare hook, uploads its paths, applies retention, then its release hook.",
      columns: [{ key: "backup", label: "" }],
      build: (picked) => ({ kind: "backup", backend: "restic", targets: picked.map(([service]) => ({ service })) }),
    },
  };
}
