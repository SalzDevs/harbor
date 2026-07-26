import { invoke } from "@tauri-apps/api/core";
import type { Account, Provider } from "./types";

export function accountLabel(account: Account): string {
  if (account.email) return account.email;
  if (account.displayName) return account.displayName;
  const name = account.provider === "gmail" ? "Gmail" : "Outlook";
  return `${name} (stub)`;
}

export async function listAccounts(): Promise<Account[]> {
  return invoke<Account[]>("list_accounts");
}

export async function addAccount(provider: Provider): Promise<Account> {
  return invoke<Account>("add_account", { provider });
}

export async function selectAccount(accountId: string): Promise<void> {
  return invoke("select_account", { accountId });
}

export async function selectedAccountId(): Promise<string | null> {
  return invoke<string | null>("selected_account_id");
}
