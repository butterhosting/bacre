/** The browser reconnects a dropped stream by itself; the listeners are told on every reconnect, to cover what happened in between */
export class ChangeClient {
  private readonly listeners = new Set<() => void>();
  private source: EventSource | undefined;

  public subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    this.source ??= this.connect();
    return () => {
      this.listeners.delete(listener);
      if (this.listeners.size === 0) {
        this.source?.close();
        this.source = undefined;
      }
    };
  }

  private connect(): EventSource {
    const source = new EventSource("/api/changes");
    let opened = false;
    const tell = () => this.listeners.forEach((listener) => listener());
    source.addEventListener("changed", tell);
    source.addEventListener("open", () => {
      // the first connect follows a fresh fetch; a later one may have missed something
      if (opened) {
        tell();
      }
      opened = true;
    });
    return source;
  }
}
