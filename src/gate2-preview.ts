import type { CoreState } from "./core-state";

const previewMode = new URLSearchParams(window.location.search).get("preview");

export const isGate2Preview =
  import.meta.env.DEV && (previewMode === "gate2" || previewMode === "gate2-pro");

export function createGate2PreviewState(): CoreState {
  const now = Math.floor(Date.now() / 1_000);
  const accountPlan = previewMode === "gate2-pro" ? "pro" : "plus";
  return {
    connection: "ready",
    error: null,
    account: {
      authenticated: true,
      requiresOpenaiAuth: false,
      accountType: "chatgpt",
      planType: accountPlan,
    },
    ordinaryUsageAllowed: true,
    resetCreditsAvailable: 1,
    stale: false,
    lastSuccessAt: now,
    windows: [
      {
        limitId: "codex",
        planType: "pro",
        sourceSlot: "primary",
        kind: "weekly",
        durationMinutes: 10_080,
        usedPercent: 42,
        remainingPercent: 58,
        resetsAt: now + 86_400,
      },
      {
        limitId: "codex",
        planType: "plus",
        sourceSlot: "secondary",
        kind: "five_hour",
        durationMinutes: 300,
        usedPercent: 28,
        remainingPercent: 72,
        resetsAt: now + 9_240,
      },
      {
        limitId: "codex",
        planType: "plus",
        sourceSlot: "primary",
        kind: "weekly",
        durationMinutes: 10_080,
        usedPercent: 42,
        remainingPercent: 58,
        resetsAt: now + 86_400,
      },
    ],
  };
}
