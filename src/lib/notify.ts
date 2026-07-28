import { invoke } from "@tauri-apps/api/core";
import type { NotifyPref } from "./types";

export async function getNotifyPref(): Promise<NotifyPref> {
  return invoke<NotifyPref>("get_notify_pref");
}

export async function setNotifyPref(pref: NotifyPref): Promise<void> {
  return invoke("set_notify_pref", { pref });
}
