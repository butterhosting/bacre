import clsx from "clsx";
import { useEffect, useRef, useState } from "react";
import { Link, useParams } from "react-router";
import { Chip } from "../comps/Chip";
import { Frame } from "../comps/Frame";
import { StatusPill } from "../comps/StatusPill";
import { Prettify } from "../helpers/Prettify";
import { useArchives } from "../hooks/useArchives";
import { useDocumentTitle } from "../hooks/useDocumentTitle";
import { useJob } from "../hooks/useJob";
import { Route } from "../Route";

export function jobPage() {
  const { id = "" } = useParams();
  const { archives } = useArchives();
  const { job, missing } = useJob(id);
  useDocumentTitle(job ? `${job.title} · Bacre` : "Job · Bacre");

  // follow the log as it grows, like a terminal
  const end = useRef<HTMLDivElement>(null);
  useEffect(() => {
    end.current?.scrollIntoView({ block: "nearest" });
  }, [job?.lines.length]);

  // the elapsed time ticks while the job runs
  const [, tick] = useState(0);
  useEffect(() => {
    if (job?.status !== "running") {
      return;
    }
    const timer = setInterval(() => tick((n) => n + 1), 1000);
    return () => clearInterval(timer);
  }, [job?.status]);

  return (
    <Frame archives={archives}>
      <div className="flex flex-col gap-1.5">
        <Link to={Route.jobs()} className="text-xs text-c-muted hover:text-c-ink">
          ← Jobs
        </Link>
        {job && (
          <>
            <h1 className="font-display text-3xl font-semibold">{job.title}</h1>
            <div className="flex flex-wrap items-center gap-2.5 text-sm text-c-ink2">
              <StatusPill status={job.status} large />
              <Chip backend={job.request.backend} />
              <span>
                {job.trigger === "schedule" ? "scheduled, started" : "started"} {Prettify.relativeDay(job.startedAt)} · {job.status === "running" ? "running for" : "took"} {Prettify.duration(job.startedAt, job.endedAt)}
              </span>
            </div>
            {job.error && <div className="text-sm font-semibold text-c-warn">{job.error}</div>}
          </>
        )}
        {missing && <h1 className="font-display text-3xl font-semibold">No such job</h1>}
      </div>

      {job && (
        <div className="flex max-h-[70vh] flex-col overflow-y-auto rounded-[10px] bg-c-ink px-4 py-3.5 font-mono text-xs leading-relaxed text-[#e8e0d4]">
          {job.lines.map((line, i) => (
            <div key={i} className={clsx("wrap-anywhere whitespace-pre-wrap", line.stream === "info" && "text-[#9c948a]", line.stream === "err" && "text-[#e0b088]")}>
              <span className="mr-3 text-[#6e655c] select-none">{Prettify.clock(line.at)}</span>
              {line.text}
            </div>
          ))}
          {job.status === "running" && <div className="animate-pulse text-c-warn">▍</div>}
          <div ref={end} />
        </div>
      )}
      {missing && <div className="text-c-muted">The daemon has no record of this job. Jobs live in memory only, so a restart forgets them.</div>}
    </Frame>
  );
}
