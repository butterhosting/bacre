import type { Archives } from "@/models/Archives";

/** How each backend is named and tinted on screen; a new backend does not compile without an entry. */
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
    /** Card title on a service page */
    card: string;
    /** Tailwind classes for the backend's chip */
    chip: string;
  };
}
