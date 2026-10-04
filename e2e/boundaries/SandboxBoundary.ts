import { access, readdir, readFile, writeFile } from "fs/promises";
import { join } from "path";
import { fileURLToPath } from "url";

/** e2e/.sandbox is mounted into the daemon's container, so a test can look into it and damage it */
export namespace SandboxBoundary {
  const root = fileURLToPath(new URL("../.sandbox/", import.meta.url));

  function live(service: string, file: string): string {
    return join(root, "disk-a", `@${service}`, file);
  }

  export async function read(service: string, file: string): Promise<string> {
    return await readFile(live(service, file), "utf-8");
  }

  export async function write(service: string, file: string, content: string): Promise<void> {
    await writeFile(live(service, file), content);
  }

  export async function exists(service: string, file: string): Promise<boolean> {
    return await access(live(service, file)).then(
      () => true,
      () => false,
    );
  }

  export async function state(service: string): Promise<string> {
    return (await readFile(join(root, "services", service, "state"), "utf-8")).trim();
  }

  export async function dumps(service: string): Promise<string[]> {
    return (await readdir(join(root, "dumps", service))).filter((name) => !name.startsWith("."));
  }

  export async function staged(service: string): Promise<string[]> {
    return await readdir(join(root, "staging", service)).catch(() => []);
  }
}
