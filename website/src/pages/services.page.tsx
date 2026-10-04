import { Archives } from "@/models/Archives";
import clsx from "clsx";
import { useState } from "react";
import { Link, useNavigate } from "react-router";
import { BackupDialog } from "../comps/BackupDialog";
import { Banner } from "../comps/Banner";
import { Button } from "../comps/Button";
import { Card } from "../comps/Card";
import { Chip } from "../comps/Chip";
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

  return (
    <Frame archives={archives}>
      <div className="flex flex-wrap items-end justify-between gap-2.5">
        <div>
          <h1 className="font-display text-3xl font-semibold">{archives ? `${archives.services.length} Services` : "Services"}</h1>
        </div>
        <div className="flex flex-wrap gap-2.5">
          {Archives.BACKENDS.map((backend) => (
            <Button key={backend} onClick={() => setDialog(backend)} disabled={candidates(backend).length === 0}>
              Backup <Chip backend={backend} />
            </Button>
          ))}
        </div>
      </div>
      {dialog && <BackupDialog backend={dialog} candidates={candidates(dialog)} onClose={() => setDialog(undefined)} />}

      {archives && <Banner errors={archives.errors} />}

      <Card>
        <table className="w-full border-collapse">
          <thead>
            <tr className="border-b border-c-line bg-c-cardhead text-left text-xs font-medium tracking-wider text-c-muted uppercase">
              <th className="px-4 py-2.5">Service</th>
              {Archives.BACKENDS.map((backend) => (
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
              <Internal.ServiceRow key={service.name} service={service} staged={archives.staged.filter((s) => s.service === service.name)} />
            ))}
            {archives?.services.length === 0 && (
              <tr>
                <td colSpan={Archives.BACKENDS.length + 2} className="px-4 py-3 text-c-muted">
                  {archives.refreshedAt ? "No bacre.yaml found in the atlas." : "Listing the archives…"}
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </Card>
    </Frame>
  );
}

namespace Internal {
  const CELL = "border-b border-c-line2 px-4 py-3 align-middle";

  type ServiceRowProps = {
    service: Archives.Service;
    /** The downloads waiting in the staging directory for this service */
    staged: Archives.Staged[];
  };
  /** The whole row opens the service; the name stays a real link so it is reachable by keyboard */
  export function ServiceRow({ service, staged }: ServiceRowProps) {
    const navigate = useNavigate();
    return (
      <tr className="cursor-pointer transition-colors last:[&>td]:border-b-0 hover:bg-c-cardhead" onClick={() => void navigate(Route.service(service.name))}>
        <td className={CELL}>
          <Link to={Route.service(service.name)} className="font-semibold text-c-ink" onClick={(e) => e.stopPropagation()}>
            {service.name}
          </Link>
        </td>
        {Archives.BACKENDS.map((backend) => (
          <td key={backend} className={clsx(CELL, "text-sm")}>
            <LatestCell service={service} backend={backend} />
            <StagedPill count={staged.filter((s) => s.backend === backend).length} />
          </td>
        ))}
        <td className={clsx(CELL, "text-right text-sm text-c-accent max-md:hidden")}>Open →</td>
      </tr>
    );
  }

  /** A quiet reminder that a download is sitting in the staging directory, waiting to be restored or discarded */
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
