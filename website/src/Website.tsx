import { createBrowserRouter, replace, RouterProvider } from "react-router";
import { ClientRegistry } from "./ClientRegistry";
import { jobPage } from "./pages/job.page";
import { jobsPage } from "./pages/jobs.page";
import { servicePage } from "./pages/service.page";
import { servicesPage } from "./pages/services.page";
import { Route } from "./Route";

const router = createBrowserRouter([
  {
    path: Route.services(),
    Component: servicesPage,
  },
  {
    path: Route.service(),
    Component: servicePage,
  },
  {
    path: Route.jobs(),
    Component: jobsPage,
  },
  {
    path: Route.job(),
    Component: jobPage,
  },
  {
    path: "*",
    loader: () => replace(Route.services()),
  },
]);

export function Website({ registry }: { registry: ClientRegistry }) {
  return (
    <ClientRegistry.Context.Provider value={registry}>
      <RouterProvider router={router} />
    </ClientRegistry.Context.Provider>
  );
}
