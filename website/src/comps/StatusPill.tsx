import type { Jobs } from "@/models/Jobs";
import clsx from "clsx";

type Props = {
  status: Jobs.Status;
  large?: boolean;
};
export function StatusPill({ status, large }: Props) {
  return (
    <span
      data-testid="status"
      className={clsx(
        "inline-flex items-center rounded-full font-semibold",
        large ? "gap-2 px-3.5 py-1.5 text-sm" : "gap-1.5 px-2.5 py-[3px] text-xs",
        status === "running" && "bg-[#f7ead3] text-[#7a5010]",
        status === "succeeded" && "bg-[#dbe9df] text-[#24503a]",
        status === "failed" && "bg-[#f1e2d9] text-[#8f3d22]",
      )}
    >
      {status === "running" && <span className={clsx("animate-spin rounded-full border-2 border-current border-t-transparent", large ? "size-3.5" : "size-3")} />}
      {status === "succeeded" && <span aria-hidden>✓</span>}
      {status === "failed" && <span aria-hidden>✕</span>}
      {status === "running" ? "busy" : status}
    </span>
  );
}
