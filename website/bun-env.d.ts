declare module "*.svg" {
  const path: `${string}.svg`;
  export = path;
}

/**
 * Imported for its side effect (the bundler injects the stylesheet); it has no shape.
 */
declare module "*.css";
