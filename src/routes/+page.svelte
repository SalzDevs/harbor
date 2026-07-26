<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import {
    accountLabel,
    addAccount,
    listAccounts,
    selectAccount,
    selectedAccountId,
  } from "$lib/accounts";
  import MessageList from "$lib/components/MessageList.svelte";
  import { folderLabel, listFolders, syncFolders } from "$lib/folders";
  import { listMessages, syncFolderHeaders } from "$lib/messages";
  import { strings } from "$lib/strings";
  import type {
    Account,
    AppInfo,
    Folder,
    FolderSyncProgress,
    MessageListItem,
    Provider,
  } from "$lib/types";

  let info = $state<AppInfo | null>(null);
  let accounts = $state<Account[]>([]);
  let folders = $state<Folder[]>([]);
  let messages = $state<MessageListItem[]>([]);
  let messageTotal = $state(0);
  let activeId = $state<string | null>(null);
  let activeFolderId = $state<string | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);
  let signingIn = $state(false);
  let syncingFolders = $state(false);
  let syncProgress = $state<FolderSyncProgress | null>(null);
  let headerSyncToken = 0;

  let unlistenProgress: UnlistenFn | null = null;

  const activeAccount = $derived(
    accounts.find((a) => a.id === activeId) ?? null,
  );
  const activeFolder = $derived(
    folders.find((f) => f.id === activeFolderId) ?? null,
  );

  onMount(async () => {
    unlistenProgress = await listen<FolderSyncProgress>(
      "folder-sync-progress",
      (event) => {
        if (event.payload.folderId === activeFolderId) {
          syncProgress = event.payload;
        }
      },
    );
    await reload();
  });

  onDestroy(() => {
    unlistenProgress?.();
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
      if (activeId) {
        await loadFolders(activeId, false);
      } else {
        folders = [];
        activeFolderId = null;
        messages = [];
        messageTotal = 0;
      }
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function loadFolders(accountId: string, fromNetwork: boolean) {
    syncingFolders = fromNetwork;
    try {
      if (fromNetwork) {
        folders = await syncFolders(accountId);
      } else {
        folders = await listFolders(accountId);
        if (folders.length === 0) {
          syncingFolders = true;
          folders = await syncFolders(accountId);
        }
      }
      const inbox = folders.find((f) => f.role === "inbox");
      const nextId = inbox?.id ?? folders[0]?.id ?? null;
      if (nextId) {
        await selectFolder(nextId);
      } else {
        activeFolderId = null;
        messages = [];
        messageTotal = 0;
      }
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      if (!fromNetwork) {
        folders = await listFolders(accountId).catch(() => []);
      }
    } finally {
      syncingFolders = false;
    }
  }

  async function selectFolder(folderId: string) {
    activeFolderId = folderId;
    syncProgress = null;
    // First paint from local DB only.
    try {
      const page = await listMessages(folderId, 300, 0);
      messages = page.messages;
      messageTotal = page.total;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      messages = [];
      messageTotal = 0;
    }
    // Background header sync.
    const token = ++headerSyncToken;
    void (async () => {
      try {
        await syncFolderHeaders(folderId);
        if (token !== headerSyncToken || activeFolderId !== folderId) return;
        const page = await listMessages(folderId, 300, 0);
        messages = page.messages;
        messageTotal = page.total;
      } catch (e) {
        if (token !== headerSyncToken || activeFolderId !== folderId) return;
        error = e instanceof Error ? e.message : String(e);
      } finally {
        if (token === headerSyncToken && activeFolderId === folderId) {
          syncProgress = null;
        }
      }
    })();
  }

  async function onAdd(provider: Provider) {
    if (provider === "gmail" && info && !info.gmailOauthConfigured) {
      error = strings.gmailOauthNotConfigured;
      return;
    }
    if (provider === "outlook" && info && !info.outlookOauthConfigured) {
      error = strings.outlookOauthNotConfigured;
      return;
    }
    busy = true;
    signingIn = true;
    error = null;
    try {
      const account = await addAccount(provider);
      accounts = await listAccounts();
      activeId = account.id;
      await loadFolders(account.id, false);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
      signingIn = false;
    }
  }

  async function onSelectAccount(id: string) {
    if (id === activeId) return;
    busy = true;
    error = null;
    try {
      await selectAccount(id);
      activeId = id;
      await loadFolders(id, false);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  async function onSyncFolders() {
    if (!activeId) return;
    error = null;
    await loadFolders(activeId, true);
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
            onclick={() => onSelectAccount(account.id)}
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

    {#if activeAccount}
      <div class="pane-label row">
        <span>{strings.folders}</span>
        <button
          type="button"
          class="linkish"
          disabled={busy || syncingFolders}
          onclick={onSyncFolders}
        >
          {syncingFolders ? strings.syncingFolders : strings.syncFolders}
        </button>
      </div>
      <div class="folder-list">
        {#if folders.length === 0}
          <p class="muted pad">{strings.noFolders}</p>
        {:else}
          {#each folders as folder (folder.id)}
            <button
              type="button"
              class="folder-item"
              class:active={folder.id === activeFolderId}
              onclick={() => selectFolder(folder.id)}
            >
              {folderLabel(folder)}
            </button>
          {/each}
        {/if}
      </div>
    {/if}

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
      {#if activeFolder}
        {folderLabel(activeFolder)}
      {:else}
        {strings.selectFolder}
      {/if}
    </div>
    {#if activeFolder}
      <MessageList {messages} total={messageTotal} {syncProgress} />
    {:else if activeAccount}
      <p class="muted pad">{accountLabel(activeAccount)}</p>
    {/if}
  </section>

  <main class="pane reading" aria-label="Reading">
    <div class="empty">
      <h1>{strings.emptyShellHeading}</h1>
      {#if activeFolder}
        <p>{strings.emptyShellBody}</p>
      {:else if activeAccount}
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
    grid-template-columns: 260px 360px 1fr;
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

  .pane-label.row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    text-transform: none;
    letter-spacing: normal;
  }

  .pane-label.row span {
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .linkish {
    border: none;
    background: transparent;
    color: var(--accent);
    font-size: 11px;
    padding: 0;
    text-transform: none;
    letter-spacing: normal;
    font-weight: 500;
  }

  .linkish:disabled {
    opacity: 0.5;
  }

  .account-list {
    max-height: 30%;
    overflow: auto;
    padding: 8px;
    flex-shrink: 0;
  }

  .folder-list {
    flex: 1;
    overflow: auto;
    padding: 8px;
  }

  .account-item,
  .folder-item {
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

  .folder-item {
    font-size: 13px;
    color: var(--text);
  }

  .account-item:hover:not(:disabled),
  .folder-item:hover {
    background: var(--bg-elevated);
  }

  .account-item.active,
  .folder-item.active {
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
