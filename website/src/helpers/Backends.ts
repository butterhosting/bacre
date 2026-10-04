import type { Archives } from "@/models/Archives";

export const Backends: Record<Archives.Backend, Backends.Copy> = {
  btrfs: {
    card: "Local snapshots",
    chip: "bg-c-btrfs-bg text-c-btrfs-fg",
  },
  restic: {
    card: "Offsite snapshots",
    chip: "bg-c-restic-bg text-c-restic-fg",
  },
};

export namespace Backends {
  export type Copy = {
    card: string;
    chip: string;
  };
}
