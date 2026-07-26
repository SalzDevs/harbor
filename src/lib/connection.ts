import { invoke } from "@tauri-apps/api/core";
import type { ConnectionStatus } from "./types";
import { strings } from "./strings";

export async function getConnectionStatus(): Promise<ConnectionStatus> {
  return invoke<ConnectionStatus>("get_connection_status");
}

export async function watchAccount(accountId: string): Promise<void> {
  return invoke("watch_account", { accountId });
}

export function connectionLabel(status: ConnectionStatus | null): string {
  if (!status) return "";
  switch (status.kind) {
    case "online":
      return status.detail
        ? `${strings.statusOnline} · ${status.detail}`
        : strings.statusOnline;
    case "offline":
      return status.detail
        ? `${strings.statusOffline} (${status.detail})`
        : strings.statusOffline;
    case "reconnecting":
      return status.detail
        ? `${strings.statusReconnecting} ${status.detail}`
        : strings.statusReconnecting;
  }
}
