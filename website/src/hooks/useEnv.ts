import type { Env } from "@/models/Env";
import { useContext } from "react";
import { ClientRegistry } from "../ClientRegistry";

/** What the server said about itself when the page started */
export function useEnv(): Env.Type {
  return useContext(ClientRegistry.Context).env;
}
