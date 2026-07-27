import { invoke } from "@tauri-apps/api/core";
import type { Contact, Draft, OutboxItem } from "./types";

export async function listDrafts(accountId: string): Promise<Draft[]> {
  return invoke<Draft[]>("list_drafts", { accountId });
}

export async function saveDraft(accountId: string, draft: Draft): Promise<void> {
  return invoke("save_draft", { accountId, draft });
}

export async function deleteDraft(draftId: string): Promise<void> {
  return invoke("delete_draft", { draftId });
}

export async function listOutbox(accountId: string): Promise<OutboxItem[]> {
  return invoke<OutboxItem[]>("list_outbox", { accountId });
}

export async function sendMail(params: {
  accountId: string;
  to: string;
  cc: string;
  bcc: string;
  subject: string;
  bodyText: string;
  bodyHtml?: string | null;
  inReplyTo?: string | null;
  references?: string | null;
}): Promise<void> {
  return invoke("send_mail", params);
}

export async function flushOutbox(): Promise<number> {
  return invoke<number>("flush_outbox");
}

export async function searchContacts(query: string, limit = 10): Promise<Contact[]> {
  return invoke<Contact[]>("search_contacts", { query, limit });
}
