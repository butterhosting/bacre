import type { Archives } from "@/models/Archives";
import type { ReactNode } from "react";
import { Prettify } from "../helpers/Prettify";

type Props = {
  info: Archives.BackendInfo;
  schedule: Archives.Schedule | undefined;
};
export function BackendInfo({ info, schedule }: Props) {
  switch (info.backend) {
    case "btrfs":
      return (
        <Internal.Facts>
          <Internal.Fact label="subvolume">
            <Internal.Path path={info.subvolume} />
          </Internal.Fact>
          <Internal.Fact label="retention">
            keep all for {info.retention.preserveMin}, then {info.retention.preserve.join(", ")}
          </Internal.Fact>
          <Internal.Schedule schedule={schedule} />
          <Internal.Fact label="destinations">
            <Internal.Blocks
              blocks={[
                { name: "snapshots", text: info.snapshots },
                { name: "targets", text: info.targets.join("\n") },
              ]}
            />
          </Internal.Fact>
          <Internal.Fact label="hooks">
            {info.lifecycle ? (
              <Internal.Blocks
                blocks={[
                  { name: "lifecycle.stop", text: info.lifecycle.stop },
                  { name: "lifecycle.start", text: info.lifecycle.start },
                ]}
              />
            ) : (
              <span className="text-c-muted">not possible, no stop and start hooks (and no btrfs restore either)</span>
            )}
          </Internal.Fact>
        </Internal.Facts>
      );
    case "restic":
      return (
        <Internal.Facts>
          <Internal.Fact label="repository">
            <Internal.Path path={info.repository} />{" "}
            <Internal.Envset name={info.envset} />
          </Internal.Fact>
          <Internal.Fact label="retention">
            keep last {info.retention.keepLast}, daily {info.retention.keepDaily}, weekly {info.retention.keepWeekly}, monthly {info.retention.keepMonthly}
          </Internal.Fact>
          <Internal.Schedule schedule={schedule} />
          <Internal.Fact label="sources">
            <Internal.Blocks blocks={[{ name: "backupPaths", text: info.paths.join("\n") }]} />
          </Internal.Fact>
          <Internal.Fact label="hooks">
            {Object.values(info.hooks).some(Boolean) ? (
              <Internal.Blocks
                blocks={[
                  { name: "lifecycle.backupPrepare", text: info.hooks.prepare },
                  { name: "lifecycle.backupRelease", text: info.hooks.release },
                  { name: "lifecycle.restoreApply", text: info.hooks.restore },
                ]}
              />
            ) : (
              <span className="text-c-muted">none</span>
            )}
          </Internal.Fact>
        </Internal.Facts>
      );
    default:
      return info satisfies never;
  }
}

namespace Internal {
  export function Facts({ children }: { children: ReactNode }) {
    return <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 border-b border-c-line bg-c-cardhead px-4 py-3 text-xs">{children}</dl>;
  }

  type FactProps = {
    label: string;
    children: ReactNode;
  };
  export function Fact({ label, children }: FactProps) {
    return (
      <>
        <dt className="pt-px text-c-muted">{label}</dt>
        <dd className="min-w-0 text-c-ink">{children}</dd>
      </>
    );
  }

  export function Schedule({ schedule }: Pick<Props, "schedule">) {
    return (
      <Fact label="schedule">
        {schedule ? (
          <>
            <code className="font-mono">{schedule.cron}</code>
{" "}
            <Tag title="The next time this schedule fires" icon={<Clock />}>
              {schedule.waiting ? "due, waiting for the running job" : schedule.next ? Prettify.relativeDay(schedule.next) : "never fires"}
            </Tag>
          </>
        ) : (
          <span className="text-c-muted">none, only by hand</span>
        )}
      </Fact>
    );
  }

  type TagProps = {
    title: string;
    icon: ReactNode;
    children: ReactNode;
  };
  function Tag({ title, icon, children }: TagProps) {
    return (
      <span
        // the negative margin keeps the tag from making its line taller than the other rows
        className="-my-1 inline-flex items-center gap-1.5 rounded-md border border-c-line bg-c-line2 py-px pr-1.5 pl-1 align-middle whitespace-nowrap text-c-ink2"
        title={title}
      >
        {icon}
        {children}
      </span>
    );
  }

  function Clock() {
    return (
      <svg viewBox="0 0 12 12" aria-hidden className="size-[13px] shrink-0">
        <circle cx="6" cy="6" r="6" className="fill-c-ink2" />
        <path d="M6 3.25V6l1.9 1.3" fill="none" className="stroke-c-card" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" />
      </svg>
    );
  }

  export function Envset({ name }: { name: string }) {
    return (
      <Tag
        title={`Runs with the "${name}" envset of the daemon config`}
        icon={
          <span aria-hidden className="rounded-[3px] bg-c-ink2 px-[3.5px] text-[9px] leading-[13px] font-bold text-c-card">
            E
          </span>
        }
      >
        <code className="font-mono">{name}</code>
      </Tag>
    );
  }

  export function Path({ path }: { path: string }) {
    return <code className="font-mono break-all">{path}</code>;
  }

  type Block = {
    name: string;
    text?: string;
  };
  export function Blocks({ blocks }: { blocks: Block[] }) {
    return (
      <div className="flex flex-col gap-2">
        {blocks
          .filter((block): block is Required<Block> => Boolean(block.text))
          .map(({ name, text }) => (
            <div key={name} className="overflow-hidden rounded-md border border-c-line bg-c-card">
              <div className="border-b border-c-line bg-c-line2 px-2.5 py-1 font-mono font-semibold text-c-ink">{name}</div>
              <pre className="px-2.5 py-1.5 font-mono wrap-anywhere whitespace-pre-wrap text-c-ink">{text.trim()}</pre>
            </div>
          ))}
      </div>
    );
  }
}
