import type { Class } from "@/types/Class";
import { useContext } from "react";
import { ClientRegistry } from "../ClientRegistry";

export function useRegistry<T>(klass: Class<T>): T {
  return useContext(ClientRegistry.Context).get(klass);
}
