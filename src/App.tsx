import { useEffect, useState } from "react";
import { ping } from "./ipc";

export default function App() {
  const [pong, setPong] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    ping()
      .then(setPong)
      .catch((e) => setError(String(e)));
  }, []);

  return (
    <main className="h-full w-full flex flex-col items-center justify-center gap-6 p-8">
      <h1 className="text-3xl font-semibold tracking-tight">Cosmos Checker</h1>
      <p className="text-sm text-slate-400">
        Массовая проверка Cosmos-кошельков (read-only).
      </p>

      <section className="rounded-lg border border-slate-800 bg-slate-900/60 px-5 py-3 font-mono text-sm">
        {error ? (
          <span className="text-red-400" data-testid="ipc-status">
            IPC error: {error}
          </span>
        ) : (
          <span data-testid="ipc-status">
            IPC: <span className="text-emerald-400">{pong ?? "…"}</span>
          </span>
        )}
      </section>

      <footer className="text-xs text-slate-500">
        Stage 1 — scaffold. См. <code>docs/CLAUDE.md</code>.
      </footer>
    </main>
  );
}
