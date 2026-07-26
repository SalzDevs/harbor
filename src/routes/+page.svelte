<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import {
    accountLabel,
    addAccount,
    listAccounts,
    selectAccount,
    selectedAccountId,
  } from "$lib/accounts";
  import { strings } from "$lib/strings";
  import type { Account, AppInfo, Provider } from "$lib/types";

  let info = $state<AppInfo | null>(null);
  let accounts = $state<Account[]>([]);
  let activeId = $state<string | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);
  let signingIn = $state(false);

  const activeAccount = $derived(
    accounts.find((a) => a.id === activeId) ?? null,
  );

  onMount(async () => {
    await reload();
  });

  async function reload() {
    error = null;
    try {
      info = await invoke<AppInfo>("app_info");
      accounts = await listAccounts();
      activeId = await selectedAccountId();
      if (!activeId && accounts.length > 0) {
        activeId = accounts[0].id;
        await selectAccount(activeId);
      }
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function onAdd(provider: Provider) {
    if (provider === "gmail" && info && !info.gmailOauthConfigured) {
      error = strings.oauthNotConfigured;
      return;
    }
    busy = true;
    signingIn = provider === "gmail";
    error = null;
    try {
      const account = await addAccount(provider);
      accounts = await listAccounts();
      activeId = account.id;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
      signingIn = false;
    }
  }

  async function onSelect(id: string) {
    if (id === activeId) return;
    busy = true;
    error = null;
    try {
      await selectAccount(id);
      activeId = id;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="shell">
  <aside class="pane folders" aria-label="Folders">
    <div class="pane-label">{strings.accounts}</div>
    <div class="account-list">
      {#if accounts.length === 0}
        <p class="muted pad">{strings.noAccounts}</p>
      {:else}
        {#each accounts as account (account.id)}
          <button
            type="button"
            class="account-item"
            class:active={account.id === activeId}
            disabled={busy}
            onclick={() => onSelect(account.id)}
          >
            <span class="provider"
              >{account.provider}
              {#if account.status === "connected"}· connected{/if}</span
            >
            <span class="label">{accountLabel(account)}</span>
          </button>
        {/each}
      {/if}
    </div>
    <div class="add-row">
      <button type="button" class="btn" disabled={busy} onclick={() => onAdd("gmail")}>
        {strings.addGmail}
      </button>
      <button type="button" class="btn" disabled={busy} onclick={() => onAdd("outlook")}>
        {strings.addOutlook}
      </button>
      {#if signingIn}
        <p class="muted hint">{strings.signingIn}</p>
      {/if}
    </div>
  </aside>

  <section class="pane list" aria-label="Messages">
    <div class="pane-label">
      {#if activeAccount}
        {strings.inbox}
      {:else}
        {strings.selectAccount}
      {/if}
    </div>
    {#if activeAccount}
      <p class="muted pad">{accountLabel(activeAccount)}</p>
    {/if}
  </section>

  <main class="pane reading" aria-label="Reading">
    <div class="empty">
      <h1>{strings.emptyShellHeading}</h1>
      {#if activeAccount}
        <p>
          {accountLabel(activeAccount)} · {activeAccount.provider} · {activeAccount.status}
        </p>
      {:else}
        <p>{strings.emptyShellBody}</p>
        <p class="status">{strings.addAccount}</p>
      {/if}
      {#if error}
        <p class="status error">{error}</p>
      {:else if info}
        <p class="status subtle">{info.dataDir}</p>
      {:else}
        <p class="status">{strings.statusLoading}</p>
      {/if}
    </div>
  </main>
</div>

<style>
  .shell {
    display: grid;
    grid-template-columns: 240px 320px 1fr;
    height: 100vh;
    width: 100vw;
    background: var(--bg);
  }

  .pane {
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    background: var(--bg-pane);
    border-right: 1px solid var(--border);
  }

  .pane:last-child {
    border-right: none;
  }

  .pane-label {
    padding: 12px 14px;
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--text-muted);
    border-bottom: 1px solid var(--border);
    background: var(--bg-elevated);
    flex-shrink: 0;
  }

  .account-list {
    flex: 1;
    overflow: auto;
    padding: 8px;
  }

  .account-item {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    width: 100%;
    margin: 0 0 4px;
    padding: 10px 12px;
    border: 1px solid transparent;
    border-radius: 8px;
    background: transparent;
    text-align: left;
  }

  .account-item:hover:not(:disabled) {
    background: var(--bg-elevated);
  }

  .account-item.active {
    background: var(--bg-elevated);
    border-color: var(--border);
  }

  .provider {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-muted);
  }

  .label {
    font-size: 13px;
    color: var(--text);
  }

  .add-row {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px;
    border-top: 1px solid var(--border);
  }

  .btn {
    width: 100%;
    padding: 8px 12px;
    border-radius: 8px;
    border: 1px solid var(--border);
    background: var(--bg-elevated);
    color: var(--text);
  }

  .btn:hover:not(:disabled) {
    border-color: var(--accent);
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .hint {
    margin: 0;
    font-size: 12px;
  }

  .reading {
    align-items: center;
    justify-content: center;
  }

  .empty {
    text-align: center;
    padding: 32px;
    max-width: 420px;
  }

  .empty h1 {
    margin: 0 0 8px;
    font-size: 28px;
    font-weight: 600;
    letter-spacing: -0.02em;
  }

  .empty p {
    margin: 0;
    color: var(--text-muted);
  }

  .pad {
    padding: 12px 14px;
  }

  .muted {
    color: var(--text-muted);
    font-size: 13px;
  }

  .status {
    margin-top: 16px !important;
    font-size: 12px;
    word-break: break-all;
  }

  .subtle {
    opacity: 0.7;
  }

  .error {
    color: #f85149;
  }
</style>
