import { Archives } from "@/models/Archives";
import type { Jobs } from "@/models/Jobs";
import clsx from "clsx";
import { useState } from "react";
import { Link, useNavigate, useParams } from "react-router";
import { JobClient } from "../clients/JobClient";
import { BackendInfo } from "../comps/BackendInfo";
import { BackupDialog } from "../comps/BackupDialog";
import { Button } from "../comps/Button";
import { Card } from "../comps/Card";
import { Chip } from "../comps/Chip";
import { ClockIcon } from "../comps/ClockIcon";
import { DiscardDialog } from "../comps/DiscardDialog";
import { Frame } from "../comps/Frame";
import { RestoreDialog } from "../comps/RestoreDialog";
import { Backends } from "../helpers/Backends";
import { Prettify } from "../helpers/Prettify";
import { useArchives } from "../hooks/useArchives";
import { useDocumentTitle } from "../hooks/useDocumentTitle";
import { useRegistry } from "../hooks/useRegistry";
import { Route } from "../Route";

export function servicePage() {
  const { name = "" } = useParams();
  useDocumentTitle(`${name} · Bacre`);
  const { archives, reload } = useArchives();
  const service = archives?.services.find((s) => s.name === name);
  const staged = archives?.staged.filter((s) => s.service === name) ?? [];
  const configured = Archives.BACKENDS.filter((backend) => service?.backends[backend] !== undefined);
  const [backup, setBackup] = useState<Archives.Backend>();
  const [restore, setRestore] = useState<Internal.Restoring>();

  return (
    <Frame actions={<h1 className="font-display text-3xl leading-[1.1] font-semibold">{name}</h1>}>
      {backup && service && <BackupDialog backend={backup} candidates={[service]} onClose={() => setBackup(undefined)} />}
      {restore && service && <RestoreDialog service={service} snapshot={restore.snapshot} staged={restore.staged} onClose={() => setRestore(undefined)} />}

      {archives && !service && <div className="text-c-muted">{archives.refreshedAt ? "No such service." : "Listing the archives…"}</div>}

      {service && (
        <div className="grid grid-cols-2 items-start gap-4.5 max-lg:grid-cols-1">
          {configured.map((backend) => (
            <Internal.BackendCard
              key={backend}
              service={service}
              backend={backend}
              staged={staged.filter((s) => s.backend === backend)}
              schedule={archives?.schedules.find((s) => s.service === name && s.backend === backend)}
              onBackup={() => setBackup(backend)}
              onRestore={setRestore}
              onStagingChanged={reload}
            />
          ))}
        </div>
      )}
    </Frame>
  );
}

namespace Internal {
  export type Restoring = {
    snapshot: Archives.Snapshot;
    staged?: Archives.Staged;
  };

  type BackendCardProps = {
    service: Archives.Service;
    backend: Archives.Backend;
    staged: Archives.Staged[];
    schedule: Archives.Schedule | undefined;
    onBackup: () => void;
    onRestore: (restoring: Restoring) => void;
    onStagingChanged: () => Promise<void>;
  };
  export function BackendCard({ service, backend, staged, schedule, onBackup, onRestore, onStagingChanged }: BackendCardProps) {
    const status = service.backends[backend]!;
    const snapshots = service.snapshots.filter((s) => s.backend === backend);
    const [open, setOpen] = useState(false);
    return (
      <Card data-testid={`${backend}-card`}>
        <Card.Head className="flex-row items-baseline">
          <Chip backend={backend} className="self-center" />
          <span className="font-semibold">Snapshots</span>
          <span className="text-xs text-c-muted">{status.state === "ok" && `(total: ${snapshots.length})`}</span>
          <span className="flex-1" />
          <button
            type="button"
            onClick={() => setOpen((o) => !o)}
            aria-label={open ? "Hide details" : "Show details"}
            aria-expanded={open}
            className={clsx(
              "flex size-6 cursor-pointer items-center justify-center self-center rounded-full border font-serif text-xs font-semibold italic transition-colors",
              open ? "border-c-ink bg-c-ink text-c-card" : "border-c-line text-c-muted hover:border-c-ink hover:text-c-ink",
            )}
          >
            i
          </button>
        </Card.Head>
        {open && <BackendInfo info={status.info} schedule={schedule} />}
        {staged.map((download) => (
          <StagedBlock
            key={download.handle}
            staged={download}
            snapshot={snapshots.find((s) => s.handle === download.handle)}
            onRestore={onRestore}
            onStagingChanged={onStagingChanged}
          />
        ))}
        <NextRow schedule={schedule} onBackup={onBackup} />
        <Body service={service} status={status} snapshots={snapshots} staged={staged} onRestore={onRestore} />
      </Card>
    );
  }

  type NextRowProps = {
    /** Absent when the backend has no schedule in its bacre.yaml */
    schedule: Archives.Schedule | undefined;
    onBackup: () => void;
  };
  /** Heads the snapshot list as the one still to come, so backing up sits where restoring does */
  function NextRow({ schedule, onBackup }: NextRowProps) {
    return (
      <div className="flex items-center gap-3 border-b border-dashed border-c-line bg-[#fdfaf4] px-4 py-2.5">
        <span className="flex size-[26px] shrink-0 items-center justify-center rounded-full border border-dashed border-[#b9ab98] text-c-muted">
          <ClockIcon dashed={!schedule} />
        </span>
        <div className="flex min-w-0 flex-col gap-0.5 text-xs">
          <span className="font-semibold">Next snapshot</span>
          <NextWhen schedule={schedule} />
        </div>
        <span className="flex-1" />
        <Button small primary onClick={onBackup}>
          Backup now
        </Button>
      </div>
    );
  }

  function NextWhen({ schedule }: { schedule: Archives.Schedule | undefined }) {
    if (!schedule) {
      return (
        <span className="text-c-muted italic" title="No schedule in the bacre.yaml; backups run only by hand">
          manual only
        </span>
      );
    }
    if (schedule.waiting) {
      return (
        <span className="font-semibold text-c-warn" title={`Scheduled as ${schedule.cron}`}>
          due, waiting for a job
        </span>
      );
    }
    return (
      <span className="text-c-muted" title={`Scheduled as ${schedule.cron}`}>
        {schedule.next ? `scheduled ${Prettify.relativeDay(schedule.next)}` : "never fires"}
      </span>
    );
  }

  type StagedBlockProps = {
    staged: Archives.Staged;
    /** Absent when the snapshot has since been pruned from the repository; the download can still be restored */
    snapshot: Archives.Snapshot | undefined;
    onRestore: (restoring: Restoring) => void;
    onStagingChanged: () => Promise<void>;
  };
  function StagedBlock({ staged, snapshot, onRestore, onStagingChanged }: StagedBlockProps) {
    const [discarding, setDiscarding] = useState(false);

    return (
      <div data-testid="staged" className="flex flex-col gap-1.5 border-b border-[#ecc9a3] bg-[#fdf6ea] px-4 pt-2.5 pb-3">
        <div className="flex flex-wrap items-center gap-2 text-xs">
          <span className="rounded-full bg-[#f7ead3] px-[7px] py-[2px] text-xs font-semibold text-[#7a5010]">staged</span>
          <code className="font-mono">{staged.handle}</code>
          <span className="text-c-muted">· downloaded {Prettify.ago(staged.downloadedAt)}</span>
          <span className="flex-1" />
          <Button small onClick={() => setDiscarding(true)}>
            Discard
          </Button>
          <Button
            small
            primary
            disabled={!snapshot}
            title={snapshot ? undefined : "This snapshot is no longer in the repository listing"}
            onClick={() => snapshot && onRestore({ snapshot, staged })}
          >
            Restore
          </Button>
        </div>
        <code className="font-mono text-xs break-all text-c-ink2">{staged.path}</code>
        {discarding && <DiscardDialog staged={staged} onDiscarded={onStagingChanged} onClose={() => setDiscarding(false)} />}
      </div>
    );
  }

  type BodyProps = {
    service: Archives.Service;
    status: Archives.BackendStatus;
    snapshots: Archives.Snapshot[];
    staged: Archives.Staged[];
    onRestore: (restoring: Restoring) => void;
  };
  function Body({ service, status, snapshots, staged, onRestore }: BodyProps) {
    switch (status.state) {
      case "absent":
        return <Card.Note>Nothing in this backend for this service yet.</Card.Note>;
      case "error":
        return (
          <Card.Note className="text-c-warn">
            Listing failed: <code className="font-mono">{status.message}</code>
          </Card.Note>
        );
      case "ok":
        return snapshots.map((snapshot) => (
          <Card.Row key={snapshot.handle} data-testid="snapshot">
            <span className={clsx("flex size-[26px] shrink-0 items-center justify-center rounded-full", Backends[snapshot.backend].chip)}>
              <svg viewBox="0 0 12 12" aria-hidden className="size-3">
                <path d="M2.5 6.4l2.3 2.3 4.7-5.2" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" />
              </svg>
            </span>
            <div className="flex min-w-0 flex-col gap-0.5">
              <code className="font-mono text-xs">{snapshot.handle}</code>
              <span className="text-xs text-c-muted">{Prettify.fullDate(snapshot.time)}</span>
              {/* on a phone the row has no room beside the handle, so the details go under it */}
              <span className="text-xs text-c-muted md:hidden">
                <Details snapshot={snapshot} />
              </span>
            </div>
            <span className="flex-1" />
            <span className="text-xs whitespace-nowrap text-c-muted max-md:hidden">
              <Details snapshot={snapshot} />
            </span>
            <Action service={service} snapshot={snapshot} staged={staged.some((s) => s.handle === snapshot.handle)} onRestore={onRestore} />
          </Card.Row>
        ));
      default:
        return status.state satisfies never;
    }
  }

  type ActionProps = {
    service: Archives.Service;
    snapshot: Archives.Snapshot;
    staged: boolean;
    onRestore: (restoring: Restoring) => void;
  };
  function Action({ service, snapshot, staged, onRestore }: ActionProps) {
    const jobClient = useRegistry(JobClient);
    const navigate = useNavigate();
    const [starting, setStarting] = useState(false);
    const [busy, setBusy] = useState<string>();

    async function download() {
      setStarting(true);
      try {
        const request: Jobs.DownloadRequest = { kind: "download", backend: "restic", service: service.name, handle: snapshot.handle };
        const job = await jobClient.start(request);
        void navigate(Route.job(job.id));
      } catch (e) {
        setBusy(e instanceof JobClient.Busy ? e.jobId : undefined);
        setStarting(false);
      }
    }

    switch (snapshot.backend) {
      case "btrfs":
        return (
          <Button small onClick={() => onRestore({ snapshot })}>
            Restore
          </Button>
        );
      case "restic":
        if (staged) {
          return <span className="px-2.5 text-xs font-medium text-[#7a5010]">staged ↑</span>;
        }
        if (busy) {
          return (
            <Link to={Route.job(busy)} className="px-2.5 text-xs font-medium text-c-warn">
              a job is running →
            </Link>
          );
        }
        return (
          <Button small onClick={() => void download()} loading={starting}>
            Download
          </Button>
        );
      default:
        return snapshot satisfies never;
    }
  }

  type DetailsProps = {
    snapshot: Archives.Snapshot;
  };
  function Details({ snapshot }: DetailsProps) {
    switch (snapshot.backend) {
      case "btrfs": {
        const { onSnapshotPaths, snapshotPaths } = snapshot.details;
        const missing = snapshotPaths - onSnapshotPaths;
        if (missing <= 0) {
          return null;
        }
        return (
          <span className="font-semibold text-c-warn">
            missing in {missing} of {snapshotPaths} snapshot paths
          </span>
        );
      }
      case "restic":
        // nothing per row: the paths are the same for every snapshot
        return null;
      default:
        return snapshot satisfies never;
    }
  }
}
