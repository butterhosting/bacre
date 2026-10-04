import clsx from "clsx";
import type { ComponentProps } from "react";

type Props = ComponentProps<"section">;
export function Card({ className, ...props }: Props) {
  return <section {...props} className={clsx("overflow-hidden rounded-[10px] border border-c-line bg-c-card", className)} />;
}

export namespace Card {
  /** The tinted strip at the top of a card: a title row, then an optional line of facts. */
  export function Head({ className, ...props }: ComponentProps<"div">) {
    return <div {...props} className={clsx("flex flex-col gap-1.5 border-b border-c-line bg-c-cardhead px-4 py-3", className)} />;
  }

  /** One entry in a card's list, separated from the next by a hairline. */
  export function Row({ className, ...props }: ComponentProps<"div">) {
    return <div {...props} className={clsx("flex items-center gap-3 border-b border-c-line2 px-4 py-2.5 last:border-b-0", className)} />;
  }

  /** A quiet closing line, e.g. "28 more". */
  export function Note({ className, ...props }: ComponentProps<"div">) {
    return <div {...props} className={clsx("px-4 py-2.5 text-xs text-c-muted", className)} />;
  }
}
