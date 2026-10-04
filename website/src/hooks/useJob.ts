import { Jobs } from "@/models/Jobs";
import { useEffect, useState } from "react";
import { JobClient } from "../clients/JobClient";
import { useRegistry } from "./useRegistry";

/**
 * One job, followed live: the event stream replays what the job printed so far and keeps
 * delivering until `done`, so the page needs no separate fetch for the lines.
 */
export function useJob(id: string): useJob.Result {
  const jobClient = useRegistry(JobClient);
  const [job, setJob] = useState<Jobs.Job>();
  const [missing, setMissing] = useState(false);

  useEffect(() => {
    let source: EventSource | undefined;
    let cancelled = false;
    setJob(undefined);
    setMissing(false);

    jobClient.get(id).then(
      (fetched) => {
        if (cancelled) {
          return;
        }
        // the stream replays every line, so start from an empty log rather than doubling up
        setJob({ ...fetched, lines: [] });
        source = jobClient.events(id);
        source.addEventListener("line", (event) => {
          const line = Jobs.Line.parse(JSON.parse((event as MessageEvent<string>).data));
          setJob((current) => (current ? { ...current, lines: [...current.lines, line] } : current));
        });
        source.addEventListener("done", (event) => {
          const { status, error } = JSON.parse((event as MessageEvent<string>).data) as { status: Jobs.Status; error: string | null };
          setJob((current) => (current ? { ...current, status, error, endedAt: current.endedAt ?? new Date().toISOString() } : current));
          source?.close();
        });
      },
      () => {
        if (!cancelled) {
          setMissing(true);
        }
      },
    );

    return () => {
      cancelled = true;
      source?.close();
    };
  }, [id]);

  return { job, missing };
}

export namespace useJob {
  export type Result = {
    job: Jobs.Job | undefined;
    missing: boolean;
  };
}
