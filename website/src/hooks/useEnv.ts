import type { Env } from "@/models/Env";
import { useContext } from "react";
import { ClientRegistry } from "../ClientRegistry";

export function useEnv(): Env.Type {
  return useContext(ClientRegistry.Context).env;
}
