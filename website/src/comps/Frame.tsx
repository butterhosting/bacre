import clsx from "clsx";
import { useState, type ComponentProps, type ReactNode } from "react";
import { Link, useLocation } from "react-router";
import { RestrictedClient } from "../clients/RestrictedClient";
import { useArchives } from "../hooks/useArchives";
import { useEnv } from "../hooks/useEnv";
import { useRegistry } from "../hooks/useRegistry";
import { Route } from "../Route";
import { Button } from "./Button";
import { LogoIcon } from "./LogoIcon";

type Props = ComponentProps<"main"> & {
  /** Buttons at the end of the tabs, on the page they belong to */
  actions?: ReactNode;
};
export function Frame({ actions, className, children, ...props }: Props) {
  return (
    <>
      <main {...props} className={clsx("mx-auto flex w-full max-w-[1120px] flex-col gap-5 px-4 pt-6 md:px-6 md:pt-10", className)}>
        <Internal.Tabs actions={actions} />
        {children}
      </main>
      <Internal.Footer />
    </>
  );
}

namespace Internal {
  type TabProps = {
    to: string;
    /** Underlined; decided by the caller, since "/" is a prefix of every path */
    active: boolean;
    children: ReactNode;
  };
  /** On the list it leads to, a tab is the page's heading; on a page below it, the way back */
  function Tab({ to, active, children }: TabProps) {
    const { pathname } = useLocation();
    const className = clsx(
      "-mb-px flex items-center gap-2.5 border-b-[3px] pb-3 font-display text-[34px] leading-[1.1] font-semibold",
      active ? "border-c-accent text-c-ink" : "border-transparent text-c-muted transition-colors hover:text-c-ink",
    );
    if (pathname === to) {
      return <h1 className={className}>{children}</h1>;
    }
    return (
      <Link to={to} className={className}>
        {children}
      </Link>
    );
  }

  type TabsProps = {
    actions: ReactNode;
  };
  export function Tabs({ actions }: TabsProps) {
    const { pathname } = useLocation();
    // a listing of its own, so every page shows the count and the tabs keep their place between pages
    const count = useArchives().archives?.services.length;
    const onJobs = pathname === Route.jobs() || pathname.startsWith(`${Route.jobs()}/`);
    return (
      <div className="flex flex-wrap items-end justify-between gap-x-3 border-b border-c-line">
        <nav className="flex gap-8">
          <Tab to={Route.services()} active={!onJobs}>
            Services {count !== undefined && <span className="rounded-full bg-c-line px-2 py-0.5 font-sans text-sm text-c-ink2">{count}</span>}
          </Tab>
          <Tab to={Route.jobs()} active={onJobs}>
            Jobs
          </Tab>
        </nav>
        {actions && <div className="flex flex-wrap gap-2.5 pb-3">{actions}</div>}
      </div>
    );
  }

  function Sandbox() {
    const env = useEnv();
    const restrictedClient = useRegistry(RestrictedClient);
    const [working, setWorking] = useState<"seed" | "purge">();
    const [problem, setProblem] = useState<string>();

    if (env.stage === "prod") {
      return null;
    }

    async function run(action: "seed" | "purge") {
      setWorking(action);
      setProblem(undefined);
      try {
        await (action === "seed" ? restrictedClient.seed() : restrictedClient.purge());
      } catch (e) {
        const response = (e as { response?: { status?: number; json?: { message?: string } } }).response;
        setProblem(response?.status === 409 ? "a job is running" : (response?.json?.message ?? "it did not work"));
      } finally {
        setWorking(undefined);
      }
    }

    return (
      <div className="flex flex-col items-center gap-2 text-xs">
        <div className="flex gap-1.5">
          <Button small onClick={() => void run("seed")} loading={working === "seed"} disabled={working !== undefined}>
            Seed
          </Button>
          <Button small onClick={() => void run("purge")} loading={working === "purge"} disabled={working !== undefined}>
            Purge
          </Button>
        </div>
        {problem && <div className="font-semibold text-c-warn">{problem}</div>}
      </div>
    );
  }

  /** The same lockup as the other Butterhost.ing products: the mark, the name, and who made it */
  export function Footer() {
    return (
      <footer className="mt-auto flex flex-col items-center gap-5 px-4 pt-14 pb-10">
        {/* the byline sits in the name's column, so "by" starts right under the "B" */}
        <div className="grid grid-cols-[auto_auto] items-center gap-x-2.5 gap-y-1">
          <LogoIcon className="size-9" />
          <span className="font-display text-4xl leading-none font-semibold">Bacre</span>
          <div className="col-start-2 text-[15px]">
            <span className="text-c-muted">by </span>
            <a
              href="https://www.butterhost.ing"
              target="_blank"
              rel="noopener noreferrer"
              className="font-semibold text-c-ink underline decoration-c-accent decoration-2 underline-offset-[3px] hover:decoration-c-ink"
            >
              Butterhost.ing
            </a>
          </div>
        </div>
        <Sandbox />
      </footer>
    );
  }
}
