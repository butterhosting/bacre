/**
 * The development server for the website: the page with hot reload on :3000, and every
 * API call passed on to the Rust server on :3001, so the page talks to one origin just
 * as it does once it is served from inside the binary.
 */
import index from "./index.html";

const BACKEND = "http://127.0.0.1:3001";

const server = Bun.serve({
  hostname: "127.0.0.1",
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
    // the stable path the Rust server serves it at too
    "/favicon.svg": Bun.file("./src/images/favicon.svg"),
    "/*": index,
  },
});

console.info(`==> Website on ${server.url} (API calls go to ${BACKEND})`);
