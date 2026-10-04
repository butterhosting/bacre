import { access, readdir, readFile, rm, writeFile } from "fs/promises";
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

  export type Disk = "disk-a" | "disk-b";

  /** The service's snapshots on a disk, oldest first */
  export async function snapshots(disk: Disk, service: string): Promise<string[]> {
    const names = await readdir(join(root, disk, ".snapshots"));
    return names.filter((name) => name.startsWith(`@${service}.`)).sort();
  }

  /** Every receive into the disk stops half way, as an interrupted one would */
  export async function failReceives(disk: Disk, failing: boolean): Promise<void> {
    const marker = join(root, disk, ".snapshots", ".fake-btrfs-fail-receive");
    await (failing ? writeFile(marker, "") : rm(marker, { force: true }));
  }

  export async function onDisk(disk: Disk, name: string): Promise<boolean> {
    return await access(join(root, disk, name)).then(
      () => true,
      () => false,
    );
  }

  export async function staged(service: string): Promise<string[]> {
    return await readdir(join(root, "staging", service)).catch(() => []);
  }
}
