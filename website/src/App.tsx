import { useEffect, useState } from "react";

type Hello = {
  message: string;
  version: string;
};

export function App() {
  const [hello, setHello] = useState<Hello>();
  const [error, setError] = useState<string>();

  useEffect(() => {
    fetch("/api/hello")
      .then(async (response) => {
        if (!response.ok) {
          throw new Error(`HTTP ${response.status}`);
        }
        setHello((await response.json()) as Hello);
      })
      .catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)));
  }, []);

  return (
    <main style={{ fontFamily: "system-ui, sans-serif", margin: "4rem auto", maxWidth: "32rem" }}>
      <h1>Hello from the website</h1>
      {hello && (
        <p>
          The server says: <strong>{hello.message}</strong> (version {hello.version})
        </p>
      )}
      {error && <p>The server did not answer: {error}</p>}
      {!hello && !error && <p>Asking the server…</p>}
    </main>
  );
}
