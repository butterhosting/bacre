import tailwind from "bun-plugin-tailwind";
import { rm } from "fs/promises";

/**
 * Builds the website into dist/, which the Rust server embeds. A build script rather than
 * the `bun build` CLI, because the Tailwind plugin can only be passed through Bun.build()
 * (bunfig's [serve.static] covers the dev server only).
 */
await rm("./dist", { recursive: true, force: true });

const result = await Bun.build({
  entrypoints: ["./index.html"],
  outdir: "./dist",
  plugins: [tailwind],
  minify: true,
  define: { "process.env.NODE_ENV": JSON.stringify("production") },
  // absolute, so the page also finds its assets when it is opened on a deep link
  publicPath: "/",
});

if (!result.success) {
  for (const log of result.logs) {
    console.error(log);
  }
  process.exit(1);
}
for (const output of result.outputs) {
  console.info(`  ${output.path.split("/").pop()}  ${(output.size / 1024).toFixed(1)} KB`);
}
