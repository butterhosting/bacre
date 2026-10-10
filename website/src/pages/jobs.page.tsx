import { useEffect, useState } from "react";
import { Link } from "react-router";
import { Card } from "../comps/Card";
import { Frame } from "../comps/Frame";
import { StatusPill } from "../comps/StatusPill";
import { Prettify } from "../helpers/Prettify";
import { useDocumentTitle } from "../hooks/useDocumentTitle";
import { useJobs } from "../hooks/useJobs";
import { Route } from "../Route";

export function jobsPage() {
  useDocumentTitle("Jobs · Bacre");
  const jobs = useJobs();

  const running = jobs?.some((job) => job.status === "running") ?? false;
  const [, tick] = useState(0);
  useEffect(() => {
    if (!running) {
      return;
    }
    const timer = setInterval(() => tick((n) => n + 1), 1000);
    return () => clearInterval(timer);
  }, [running]);

  return (
    <Frame>
      <Card>
        {jobs?.map((job) => (
          <Card.Row key={job.id} className="max-md:flex-wrap max-md:gap-y-1">
            <StatusPill status={job.status} />
            <div className="flex min-w-0 flex-col gap-0.5">
              <Link to={Route.job(job.id)} className="font-medium text-c-ink hover:text-c-accent">
                {job.title}
              </Link>
              {job.error && <span className="truncate text-xs text-c-warn">{job.error}</span>}
            </div>
            <span className="flex-1 max-md:hidden" />
            <span className="text-xs whitespace-nowrap text-c-muted max-md:basis-full">
              {job.trigger === "schedule" && "scheduled · "}
              {Prettify.relativeDay(job.startedAt)} · {Prettify.duration(job.startedAt, job.endedAt)}
            </span>
          </Card.Row>
        ))}
        {jobs?.length === 0 && <Card.Note>No jobs since Bacre was last restarted</Card.Note>}
      </Card>
    </Frame>
  );
}
