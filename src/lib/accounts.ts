import { invoke } from "@tauri-apps/api/core";
import type { Account, Provider } from "./types";

export function accountLabel(account: Account): string {
  if (account.email) return account.email;
  if (account.displayName) return account.displayName;
  const name = account.provider === "gmail" ? "Gmail" : "Outlook";
  return `${name} (${account.status})`;
}

export async function listAccounts(): Promise<Account[]> {
  return invoke<Account[]>("list_accounts");
}

export async function addAccount(provider: Provider): Promise<Account> {
  if (provider === "gmail") {
    return invoke<Account>("sign_in_gmail_account");
  }
  return invoke<Account>("sign_in_outlook_account");
}

export async function selectAccount(accountId: string): Promise<void> {
  return invoke("select_account", { accountId });
}

export async function selectedAccountId(): Promise<string | null> {
  return invoke<string | null>("selected_account_id");
}
