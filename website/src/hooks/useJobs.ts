import type { Jobs } from "@/models/Jobs";
import { useYesQuery } from "react-yesquery";
import { JobClient } from "../clients/JobClient";
import { useChanges } from "./useChanges";
import { useRegistry } from "./useRegistry";

/** The recent jobs, kept current: it fetches again whenever the server reports a change */
export function useJobs(): Jobs.Summary[] | undefined {
  const jobClient = useRegistry(JobClient);
  const { data, reload } = useYesQuery({ queryFn: () => jobClient.list() });
  useChanges(() => void reload());

  return data;
}
