import type { Archives } from "@/models/Archives";
import { useYesQuery } from "react-yesquery";
import { ArchiveClient } from "../clients/ArchiveClient";
import { useChanges } from "./useChanges";
import { useRegistry } from "./useRegistry";

/** What the server knows about the archives, kept current: it fetches again whenever the server reports a change */
export function useArchives(): useArchives.Result {
  const archiveClient = useRegistry(ArchiveClient);
  const { data, reload } = useYesQuery({ queryFn: () => archiveClient.get() });
  useChanges(() => void reload());

  return { archives: data, reload: async () => void (await reload()) };
}

export namespace useArchives {
  export type Result = {
    archives: Archives.Type | undefined;
    /** Fetches again now, for a change this page made itself (a discarded download) */
    reload(): Promise<void>;
  };
}
