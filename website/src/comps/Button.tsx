import clsx from "clsx";
import type { ComponentProps } from "react";

type Props = ComponentProps<"button"> & {
  primary?: boolean;
  small?: boolean;
  loading?: boolean;
};
export function Button({ primary, small, loading, disabled, className, children, ...props }: Props) {
  return (
    <button
      {...props}
      disabled={disabled || loading}
      className={clsx(
        "inline-flex shrink-0 cursor-pointer items-center gap-2 border font-semibold leading-none",
        small ? "rounded-md px-2.5 py-1.5 text-xs" : "rounded-lg px-3.5 py-2 text-sm",
        "transition-colors disabled:cursor-not-allowed disabled:opacity-50",
        primary ? "border-c-accent bg-c-accent text-c-card hover:brightness-110" : "border-[#d8cbb9] bg-c-card text-c-ink hover:bg-c-cardhead",
        className,
      )}
    >
      {loading && <span className="size-3.5 animate-spin rounded-full border-2 border-current border-t-transparent" />}
      {children}
    </button>
  );
}
