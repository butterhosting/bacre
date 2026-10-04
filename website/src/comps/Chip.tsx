import type { Archives } from "@/models/Archives";
import clsx from "clsx";
import { Backends } from "../helpers/Backends";

type Props = {
  backend: Archives.Backend;
  className?: string;
};
export function Chip({ backend, className }: Props) {
  return (
    <span className={clsx("inline-block rounded-full px-[7px] py-[2px] text-xs font-semibold leading-tight", Backends[backend].chip, className)}>
      {backend}
    </span>
  );
}
