import { invoke } from "@tauri-apps/api/core";
import type { Folder } from "./types";

export function folderLabel(folder: Folder): string {
  switch (folder.role) {
    case "inbox":
      return "Inbox";
    case "sent":
      return "Sent";
    case "drafts":
      return "Drafts";
    case "trash":
      return "Trash";
    case "junk":
      return "Junk";
    case "archive":
      return "Archive";
    default:
      return folder.name;
  }
}

export async function listFolders(accountId: string): Promise<Folder[]> {
  return invoke<Folder[]>("list_folders", { accountId });
}

export async function syncFolders(accountId: string): Promise<Folder[]> {
  return invoke<Folder[]>("sync_folders", { accountId });
}
