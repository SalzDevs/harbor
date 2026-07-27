import { invoke } from "@tauri-apps/api/core";
import type {
  ActionRecord,
  ConversationPage,
  MessageDetail,
  MessageListItem,
  MessagePage,
  ViewMode,
} from "./types";

export function messageFrom(msg: MessageListItem): string {
  if (msg.fromName) return msg.fromName;
  if (msg.fromAddress) return msg.fromAddress;
  return "(unknown)";
}

export function messageSubject(msg: MessageListItem): string {
  const s = msg.subject?.trim();
  return s ? s : "(no subject)";
}

export function formatMessageDate(dateUnix: number): string {
  if (!dateUnix) return "";
  const d = new Date(dateUnix * 1000);
  const now = new Date();
  const sameDay =
    d.getFullYear() === now.getFullYear() &&
    d.getMonth() === now.getMonth() &&
    d.getDate() === now.getDate();
  if (sameDay) {
    return d.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
  }
  if (d.getFullYear() === now.getFullYear()) {
    return d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
  }
  return d.toLocaleDateString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}

export async function listMessages(
  folderId: string,
  limit = 200,
  offset = 0,
): Promise<MessagePage> {
  return invoke<MessagePage>("list_messages", { folderId, limit, offset });
}

export async function syncFolderHeaders(folderId: string): Promise<{
  folderId: string;
  fetched: number;
  total: number;
}> {
  return invoke("sync_folder_headers", { folderId });
}

export async function openMessage(
  folderId: string,
  messageId: string,
): Promise<MessageDetail> {
  return invoke<MessageDetail>("open_message", { folderId, messageId });
}

export async function setMessageFlags(
  folderId: string,
  messageId: string,
  seen?: boolean,
  flagged?: boolean,
): Promise<ActionRecord> {
  return invoke<ActionRecord>("set_message_flags", { folderId, messageId, seen, flagged });
}

export async function archiveMessage(
  folderId: string,
  messageId: string,
): Promise<ActionRecord> {
  return invoke<ActionRecord>("archive_message", { folderId, messageId });
}

export async function deleteMessage(
  folderId: string,
  messageId: string,
): Promise<ActionRecord> {
  return invoke<ActionRecord>("delete_message", { folderId, messageId });
}

export async function moveMessage(
  folderId: string,
  messageId: string,
  destFolderId: string,
): Promise<ActionRecord> {
  return invoke<ActionRecord>("move_message", { folderId, messageId, destFolderId });
}

export async function undoAction(actionId: string): Promise<void> {
  return invoke("undo_action", { actionId });
}

// --- Conversations + view mode ---

export async function listConversations(
  folderId: string,
  limit = 200,
  offset = 0,
): Promise<ConversationPage> {
  return invoke<ConversationPage>("list_conversations", { folderId, limit, offset });
}

export async function listThreadMessages(
  folderId: string,
  threadRoot: string,
): Promise<MessageListItem[]> {
  return invoke<MessageListItem[]>("list_thread_messages", { folderId, threadRoot });
}

export async function getViewMode(): Promise<ViewMode> {
  return invoke<ViewMode>("get_view_mode");
}

export async function setViewMode(mode: ViewMode): Promise<void> {
  return invoke("set_view_mode", { mode });
}
