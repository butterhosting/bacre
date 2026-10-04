import type { Archives } from "@/models/Archives";
import { useYesQuery } from "react-yesquery";
import { ArchiveClient } from "../clients/ArchiveClient";
import { useChanges } from "./useChanges";
import { useRegistry } from "./useRegistry";

export function useArchives(): useArchives.Result {
  const archiveClient = useRegistry(ArchiveClient);
  const { data, reload } = useYesQuery({ queryFn: () => archiveClient.get() });
  useChanges(() => void reload());

  return { archives: data, reload: async () => void (await reload()) };
}

export namespace useArchives {
  export type Result = {
    archives: Archives.Type | undefined;
    reload(): Promise<void>;
  };
}
