import clsx from "clsx";

type Props = {
  /** An empty, dashed face for when nothing is scheduled */
  dashed?: boolean;
  className?: string;
};
/** Takes the text color, so it reads as muted, warning or ink wherever it sits */
export function ClockIcon({ dashed = false, className }: Props) {
  return (
    <svg viewBox="0 0 12 12" aria-hidden className={clsx("size-[13px] shrink-0", className)}>
      {dashed ? (
        <circle cx="6" cy="6" r="5.25" fill="none" stroke="currentColor" strokeWidth="1.3" strokeDasharray="2 2" />
      ) : (
        <>
          <circle cx="6" cy="6" r="6" fill="currentColor" />
          <path d="M6 3.25V6l1.9 1.3" fill="none" className="stroke-c-card" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" />
        </>
      )}
    </svg>
  );
}
