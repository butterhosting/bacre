import type { Archives } from "@/models/Archives";
import clsx from "clsx";
import type { ComponentProps, ReactNode } from "react";
import { Link, useLocation } from "react-router";
import { Route } from "../Route";
import { LogoIcon } from "./LogoIcon";

type Props = ComponentProps<"main"> & {
  /** The page's own listing, so the sidebar shows the same freshness the page does. */
  archives: Archives.Type | undefined;
};
/** Sidebar plus content column; every page sits inside one. */
export function Frame({ archives, className, ...props }: Props) {
  return (
    <>
      <Internal.Nav archives={archives} />
      <main {...props} className={clsx("flex min-w-0 flex-1 flex-col gap-5 px-4 py-5 md:px-9 md:py-7", className)} />
    </>
  );
}

namespace Internal {
  type ItemProps = {
    to: string;
    /** Highlighted; decided by the caller, since "/" is a prefix of every path */
    active: boolean;
    children: ReactNode;
  };
  function Item({ to, active, children }: ItemProps) {
    return (
      <Link to={to} className={clsx("rounded-lg px-3 py-2", active ? "border border-c-line bg-c-card font-semibold text-c-ink" : "text-c-ink2")}>
        {children}
      </Link>
    );
  }

  type NavProps = Pick<Props, "archives">;
  export function Nav({ archives }: NavProps) {
    const { pathname } = useLocation();
    const onJobs = pathname === Route.jobs() || pathname.startsWith(`${Route.jobs()}/`);
    return (
      <nav className="flex shrink-0 items-center gap-3 border-b border-c-line bg-c-nav px-4 py-3 md:w-[220px] md:flex-col md:items-stretch md:gap-7 md:border-r md:border-b-0 md:px-5 md:py-6">
        <Link to={Route.services()} className="flex items-center gap-2.5 text-c-ink">
          <LogoIcon className="size-[34px]" />
          <div>
            <div className="font-display text-2xl leading-none font-semibold">Bacre</div>
            <div className="text-xs tracking-wide text-c-muted uppercase max-md:hidden">backup · restore</div>
          </div>
        </Link>
        <div className="flex gap-1 max-md:ml-auto md:flex-col">
          <Item to={Route.services()} active={!onJobs}>
            Services
          </Item>
          <Item to={Route.jobs()} active={onJobs}>
            Jobs
          </Item>
        </div>
        <div className="mt-auto flex flex-col gap-1.5 text-xs text-c-muted max-md:hidden">
          <div className="font-mono text-c-ink">{window.location.hostname}</div>
          {archives && archives.errors.length > 0 && (
            <div className="font-semibold text-c-warn">
              {archives.errors.length} {archives.errors.length === 1 ? "listing" : "listings"} failed
            </div>
          )}
        </div>
      </nav>
    );
  }
}
