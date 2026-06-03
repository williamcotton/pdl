import React from "react";

import { loadPdlRuntime, type PdlRuntime } from "../../pdlWasm";

export type RuntimeState = "loading" | "ready" | "error";

export interface RuntimeHandle {
  runtime: PdlRuntime | null;
  state: RuntimeState;
  error: string | null;
}

let runtimePromise: Promise<PdlRuntime> | null = null;

function sharedRuntime(): Promise<PdlRuntime> {
  if (!runtimePromise) {
    runtimePromise = loadPdlRuntime().catch((error) => {
      runtimePromise = null;
      throw error;
    });
  }
  return runtimePromise;
}

export function usePdlRuntime(): RuntimeHandle {
  const [runtime, setRuntime] = React.useState<PdlRuntime | null>(null);
  const [state, setState] = React.useState<RuntimeState>("loading");
  const [error, setError] = React.useState<string | null>(null);

  React.useEffect(() => {
    let cancelled = false;
    setState("loading");
    sharedRuntime()
      .then((loaded) => {
        if (cancelled) return;
        setRuntime(loaded);
        setState("ready");
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setError(err instanceof Error ? err.message : String(err));
        setState("error");
      });
    return () => {
      cancelled = true;
    };
  }, []);

  return { runtime, state, error };
}
