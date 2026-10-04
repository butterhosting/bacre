import { createRoot } from "react-dom/client";
import { ClientRegistry } from "./ClientRegistry";
import { Website } from "./Website";
import "./index.css";

const RETRY_MS = 1_000;

/** A server that does not answer yet (still compiling, restarting after a deploy) is waited for */
async function start() {
  const root = createRoot(document.getElementById("root")!);
  for (;;) {
    try {
      const registry = await ClientRegistry.bootstrap();
      root.render(<Website registry={registry} />);
      return;
    } catch (e) {
      if (!isUnreachable(e)) {
        root.render(<Notice>The server answered with an error: {describe(e)}</Notice>);
        return;
      }
      root.render(<Notice>Waiting for the server…</Notice>);
      await new Promise((resolve) => setTimeout(resolve, RETRY_MS));
    }
  }
}

function isUnreachable(e: unknown): boolean {
  const status = (e as { response?: { status?: number } }).response?.status;
  return status === undefined || status === 502 || status === 503 || status === 504;
}

function describe(e: unknown): string {
  const status = (e as { response?: { status?: number } }).response?.status;
  return status ? `HTTP ${status}` : e instanceof Error ? e.message : String(e);
}

function Notice({ children }: { children: React.ReactNode }) {
  return <div className="m-auto p-6 text-sm text-c-muted">{children}</div>;
}

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", () => void start());
} else {
  void start();
}
