import type { Archives } from "@/models/Archives";

type Props = {
  errors: Archives.Problem[];
};
export function Banner({ errors }: Props) {
  if (errors.length === 0) {
    return null;
  }
  return (
    <div className="flex flex-col gap-3">
      {errors.map((problem, i) => (
        <div key={i} className="overflow-hidden rounded-[10px] border border-c-accent bg-c-card">
          <div className="flex items-center gap-3 border-b border-c-accent/30 bg-c-accent/10 px-4 py-2">
            <span className="rounded-full bg-c-accent px-2 py-[2px] text-[11px] font-semibold tracking-wider text-c-card uppercase">failed</span>
            <code className="truncate font-mono text-xs text-c-ink2">{problem.source}</code>
          </div>
          <code className="block px-4 py-3 font-mono text-xs whitespace-pre-wrap text-c-ink">{problem.message}</code>
        </div>
      ))}
    </div>
  );
}
