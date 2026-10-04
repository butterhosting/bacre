import clsx from "clsx";
import { useState, type ComponentProps, type ReactNode } from "react";
import { Link, useLocation } from "react-router";
import { RestrictedClient } from "../clients/RestrictedClient";
import { useEnv } from "../hooks/useEnv";
import { useRegistry } from "../hooks/useRegistry";
import { Route } from "../Route";
import { Button } from "./Button";
import { LogoIcon } from "./LogoIcon";

export function Frame({ className, ...props }: ComponentProps<"main">) {
  return (
    <>
      <Internal.Nav />
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
      <Link
        to={to}
        className={clsx("rounded-lg px-3 py-2", active ? "border border-c-line bg-c-card font-semibold text-c-ink" : "text-c-ink2")}
      >
        {children}
      </Link>
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
      <div className="mt-3 flex flex-col gap-2 border-t border-c-line pt-3">
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

  export function Nav() {
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
          <Sandbox />
        </div>
      </nav>
    );
  }
}
