/** Hot reload on :3000, and /api passed on to the Rust server: one origin, as in production */
import index from "./index.html";

const BACKEND = process.env.BACRE_BACKEND ?? "http://127.0.0.1:3001";

const server = Bun.serve({
  hostname: process.env.BACRE_WEBSITE_HOST ?? "127.0.0.1",
  port: 3000,
  development: { hmr: true, console: true },
  // event streams stay open
  idleTimeout: 0,
  routes: {
    "/api/*": async (request) => {
      const { pathname, search } = new URL(request.url);
      try {
        return await fetch(new Request(`${BACKEND}${pathname}${search}`, request));
      } catch {
        return Response.json({ error: "backend_unreachable", backend: BACKEND }, { status: 502 });
      }
    },
    "/favicon.svg": Bun.file("./src/images/favicon.svg"),
    "/*": index,
  },
});

console.info(`==> Website on ${server.url} (API calls go to ${BACKEND})`);
