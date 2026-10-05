import type { Archives } from "@/models/Archives";

export const Backends: Record<Archives.Backend, Backends.Copy> = {
  btrfs: {
    chip: "bg-c-btrfs-bg text-c-btrfs-fg",
  },
  restic: {
    chip: "bg-c-restic-bg text-c-restic-fg",
  },
};

export namespace Backends {
  export type Copy = {
    chip: string;
  };
}
