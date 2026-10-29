import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";
import type { CoreState } from "./core-state";

interface CoreStateView {
  state: CoreState | null;
  loading: boolean;
  refreshing: boolean;
  bridgeUnavailable: boolean;
  refresh: () => Promise<void>;
}

export function useCoreState(): CoreStateView {
  const [state, setState] = useState<CoreState | null>(null);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [bridgeUnavailable, setBridgeUnavailable] = useState(false);

  useEffect(() => {
    let disposed = false;
    let removeListener: (() => void) | undefined;

    async function connect() {
      try {
        const unlisten = await listen<CoreState>("core-state", (event) => {
          if (!disposed) {
            setState(event.payload);
            setBridgeUnavailable(false);
          }
        });

        if (disposed) {
          unlisten();
          return;
        }
        removeListener = unlisten;

        const snapshot = await invoke<CoreState>("get_core_state");
        if (!disposed) {
          setState(snapshot);
          setBridgeUnavailable(false);
        }
      } catch {
        if (!disposed) {
          setBridgeUnavailable(true);
        }
      } finally {
        if (!disposed) {
          setLoading(false);
        }
      }
    }

    void connect();
    return () => {
      disposed = true;
      removeListener?.();
    };
  }, []);

  const refresh = useCallback(async () => {
    setRefreshing(true);
    try {
      const snapshot = await invoke<CoreState>("refresh_core");
      setState(snapshot);
      setBridgeUnavailable(false);
    } catch {
      setBridgeUnavailable(true);
    } finally {
      setRefreshing(false);
    }
  }, []);

  return { state, loading, refreshing, bridgeUnavailable, refresh };
}

