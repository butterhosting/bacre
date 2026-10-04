import type { Jobs } from "@/models/Jobs";
import { useYesQuery } from "react-yesquery";
import { JobClient } from "../clients/JobClient";
import { useChanges } from "./useChanges";
import { useRegistry } from "./useRegistry";

export function useJobs(): Jobs.Summary[] | undefined {
  const jobClient = useRegistry(JobClient);
  const { data, reload } = useYesQuery({ queryFn: () => jobClient.list() });
  useChanges(() => void reload());

  return data;
}
