export const Route = {
  services() {
    return "/";
  },
  service(name = ":name") {
    return `/services/${name}`;
  },
  jobs() {
    return "/jobs";
  },
  job(id = ":id") {
    return `/jobs/${id}`;
  },
};
