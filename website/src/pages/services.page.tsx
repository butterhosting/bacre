import { Archives } from "@/models/Archives";
import clsx from "clsx";
import { useState } from "react";
import { Link, useNavigate } from "react-router";
import { BackupDialog } from "../comps/BackupDialog";
import { Banner } from "../comps/Banner";
import { Button } from "../comps/Button";
import { Card } from "../comps/Card";
import { Chip } from "../comps/Chip";
import { ClockIcon } from "../comps/ClockIcon";
import { Frame } from "../comps/Frame";
import { Prettify } from "../helpers/Prettify";
import { useArchives } from "../hooks/useArchives";
import { useDocumentTitle } from "../hooks/useDocumentTitle";
import { Route } from "../Route";

export function servicesPage() {
  useDocumentTitle("Services · Bacre");
  const { archives } = useArchives();
  const [dialog, setDialog] = useState<Archives.Backend>();
  const candidates = (backend: Archives.Backend) => archives?.services.filter((s) => s.backends[backend] !== undefined) ?? [];
  // a column no service backs up to would stay empty, so it only shows while there is nothing to judge by yet
  const columns = Archives.BACKENDS.filter((backend) => !archives?.services.length || candidates(backend).length > 0);

  return (
    <Frame
      actions={Archives.BACKENDS.map((backend) => (
        <Button key={backend} onClick={() => setDialog(backend)} disabled={candidates(backend).length === 0}>
          Backup <Chip backend={backend} />
        </Button>
      ))}
    >
      {dialog && <BackupDialog backend={dialog} candidates={candidates(dialog)} onClose={() => setDialog(undefined)} />}

      {archives && <Banner errors={archives.errors} />}

      <Card>
        {/* the card clips its corners, so a phone too narrow for the table scrolls it here */}
        <div className="overflow-x-auto">
          <table className="w-full border-collapse">
            <thead>
              <tr className="border-b border-c-line bg-c-cardhead text-left text-xs font-medium tracking-wider text-c-muted uppercase">
                <th className="px-4 py-2.5">Service</th>
                {columns.map((backend) => (
                  <th key={backend} className="px-4 py-2.5 whitespace-nowrap">
                    Latest
                    <Chip backend={backend} className="ml-2 tracking-normal normal-case" />
                  </th>
                ))}
                <th className="px-4 py-2.5 max-md:hidden"></th>
              </tr>
            </thead>
            <tbody>
              {archives?.services.map((service) => (
                <Internal.ServiceRow
                  key={service.name}
                  service={service}
                  columns={columns}
                  staged={archives.staged.filter((s) => s.service === service.name)}
                  schedules={archives.schedules.filter((s) => s.service === service.name)}
                />
              ))}
              {archives?.services.length === 0 && (
                <tr>
                  <td colSpan={columns.length + 2} className="px-4 py-3 text-c-muted">
                    {archives.refreshedAt ? "No bacre.yaml found in the atlas." : "Listing the archives…"}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </Card>
    </Frame>
  );
}

namespace Internal {
  const CELL = "border-b border-c-line2 px-4 py-3 align-middle";

  type ServiceRowProps = {
    service: Archives.Service;
    columns: Archives.Backend[];
    staged: Archives.Staged[];
    schedules: Archives.Schedule[];
  };
  /** The whole row opens the service; the name stays a real link so it is reachable by keyboard */
  export function ServiceRow({ service, columns, staged, schedules }: ServiceRowProps) {
    const navigate = useNavigate();
    return (
      <tr className="cursor-pointer transition-colors last:[&>td]:border-b-0 hover:bg-c-cardhead" onClick={() => void navigate(Route.service(service.name))}>
        <td className={CELL}>
          <Link to={Route.service(service.name)} className="font-semibold text-c-ink" onClick={(e) => e.stopPropagation()}>
            {service.name}
          </Link>
        </td>
        {columns.map((backend) => (
          <td key={backend} className={clsx(CELL, "text-sm")}>
            {service.backends[backend] && (
              <div className="flex flex-col gap-0.5">
                <div>
                  <LatestCell service={service} backend={backend} />
                  <StagedPill count={staged.filter((s) => s.backend === backend).length} />
                </div>
                <NextLine schedule={schedules.find((s) => s.backend === backend)} />
              </div>
            )}
          </td>
        ))}
        <td className={clsx(CELL, "text-right text-sm text-c-accent max-md:hidden")}>Open →</td>
      </tr>
    );
  }

  function StagedPill({ count }: { count: number }) {
    if (count === 0) {
      return null;
    }
    return (
      <span className="ml-2.5 rounded-full bg-[#f7ead3] px-[7px] py-[2px] text-xs font-semibold text-[#7a5010]" title="Downloaded and waiting in the staging directory">
        {count === 1 ? "staged" : `${count} staged`}
      </span>
    );
  }

  type NextLineProps = {
    /** Absent when the backend has no schedule in its bacre.yaml */
    schedule: Archives.Schedule | undefined;
  };
  function NextLine({ schedule }: NextLineProps) {
    if (!schedule) {
      return (
        <span className="flex items-center gap-1.5 text-xs whitespace-nowrap text-c-muted italic" title="No schedule in the bacre.yaml; backups run only by hand">
          <ClockIcon dashed />
          manual only
        </span>
      );
    }
    if (schedule.waiting) {
      return (
        <span className="flex items-center gap-1.5 text-xs font-semibold text-c-warn" title={`Scheduled as ${schedule.cron}`}>
          <ClockIcon />
          due, waiting for a job
        </span>
      );
    }
    return (
      <span className="flex items-center gap-1.5 text-xs whitespace-nowrap text-c-muted" title={`Scheduled as ${schedule.cron}`}>
        <ClockIcon />
        {schedule.next ? (
          <span>
            {/* on a phone the column is too narrow for the word, and the clock says it already */}
            <span className="max-md:hidden">next </span>
            {Prettify.relativeDay(schedule.next)}
          </span>
        ) : (
          "never fires"
        )}
      </span>
    );
  }

  type LatestCellProps = {
    service: Archives.Service;
    backend: Archives.Backend;
  };
  function LatestCell({ service, backend }: LatestCellProps) {
    const status = service.backends[backend];
    if (!status) {
      return null;
    }
    switch (status.state) {
      case "absent":
        return <span className="text-c-muted">no snapshots</span>;
      case "error":
        return <span className="font-semibold text-c-warn">listing failed</span>;
      case "ok": {
        const latest = service.snapshots.find((s) => s.backend === backend);
        if (!latest) {
          return <span className="text-c-muted">no snapshots</span>;
        }
        return Prettify.isStale(latest.time) ? (
          <span className="font-semibold text-c-warn">{Prettify.relativeDay(latest.time)} · stale</span>
        ) : (
          Prettify.relativeDay(latest.time)
        );
      }
      default:
        return status.state satisfies never;
    }
  }
}
