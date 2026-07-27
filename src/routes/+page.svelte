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
  import ReadingPane from "$lib/components/ReadingPane.svelte";
  import UndoBar from "$lib/components/UndoBar.svelte";
  import {
    connectionLabel,
    getConnectionStatus,
    watchAccount,
  } from "$lib/connection";
  import { folderLabel, listFolders, syncFolders } from "$lib/folders";
  import {
    archiveMessage,
    deleteMessage,
    listMessages,
    openMessage,
    setMessageFlags,
    syncFolderHeaders,
    undoAction,
  } from "$lib/messages";
  import { strings } from "$lib/strings";
  import type {
    Account,
    ActionRecord,
    AppInfo,
    ConnectionStatus,
    Folder,
    FolderMailUpdated,
    FolderSyncProgress,
    MessageDetail,
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
  let selectedMessageId = $state<string | null>(null);
  let openDetail = $state<MessageDetail | null>(null);
  let openLoading = $state(false);
  let openError = $state<string | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);
  let signingIn = $state(false);
  let syncingFolders = $state(false);
  let syncProgress = $state<FolderSyncProgress | null>(null);
  let connection = $state<ConnectionStatus | null>(null);
  let lastAction = $state<ActionRecord | null>(null);
  let headerSyncToken = 0;
  let openToken = 0;

  let unlistenProgress: UnlistenFn | null = null;
  let unlistenStatus: UnlistenFn | null = null;
  let unlistenMail: UnlistenFn | null = null;

  const activeAccount = $derived(
    accounts.find((a) => a.id === activeId) ?? null,
  );
  const activeFolder = $derived(
    folders.find((f) => f.id === activeFolderId) ?? null,
  );
  const statusText = $derived(connectionLabel(connection));

  onMount(async () => {
    unlistenProgress = await listen<FolderSyncProgress>(
      "folder-sync-progress",
      (event) => {
        if (event.payload.folderId === activeFolderId) {
          syncProgress = event.payload;
        }
      },
    );
    unlistenStatus = await listen<ConnectionStatus>(
      "connection-status",
      (event) => {
        connection = event.payload;
      },
    );
    unlistenMail = await listen<FolderMailUpdated>(
      "folder-mail-updated",
      async (event) => {
        if (event.payload.folderId === activeFolderId) {
          try {
            const page = await listMessages(event.payload.folderId, 300, 0);
            messages = page.messages;
            messageTotal = page.total;
          } catch {
            /* keep cached list */
          }
        }
      },
    );
    try {
      connection = await getConnectionStatus();
    } catch {
      /* ignore */
    }
    await reload();
  });

  onDestroy(() => {
    unlistenProgress?.();
    unlistenStatus?.();
    unlistenMail?.();
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
      } else if (activeId) {
        await watchAccount(activeId).catch(() => undefined);
      }
      if (activeId) {
        await loadFolders(activeId, false);
      } else {
        folders = [];
        activeFolderId = null;
        messages = [];
        messageTotal = 0;
        clearOpen();
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
        clearOpen();
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

  function clearOpen() {
    selectedMessageId = null;
    openDetail = null;
    openLoading = false;
    openError = null;
  }

  async function selectFolder(folderId: string) {
    activeFolderId = folderId;
    syncProgress = null;
    clearOpen();
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

  async function onSelectMessage(msg: MessageListItem) {
    if (!activeFolderId) return;
    selectedMessageId = msg.id;
    openLoading = true;
    openError = null;
    openDetail = null;
    const token = ++openToken;
    try {
      const detail = await openMessage(activeFolderId, msg.id);
      if (token !== openToken) return;
      openDetail = detail;
    } catch (e) {
      if (token !== openToken) return;
      openError = e instanceof Error ? e.message : String(e);
    } finally {
      if (token === openToken) openLoading = false;
    }
  }

  async function refreshFolderList(folderId: string) {
    try {
      const page = await listMessages(folderId, 300, 0);
      messages = page.messages;
      messageTotal = page.total;
    } catch {
      /* keep cached */
    }
  }

  async function reloadOpenDetail() {
    if (!activeFolderId || !selectedMessageId) return;
    try {
      openDetail = await openMessage(activeFolderId, selectedMessageId);
    } catch {
      openDetail = null;
    }
  }

  async function onMessageAction(
    kind: "toggleRead" | "toggleStar" | "archive" | "delete",
  ) {
    if (!activeFolderId || !openDetail) return;
    const folderId = activeFolderId;
    const messageId = openDetail.id;
    try {
      if (kind === "toggleRead") {
        const record = await setMessageFlags(folderId, messageId, !openDetail.flags.seen);
        lastAction = record;
        openDetail = { ...openDetail, flags: { ...openDetail.flags, seen: !openDetail.flags.seen } };
        await refreshFolderList(folderId);
      } else if (kind === "toggleStar") {
        const record = await setMessageFlags(folderId, messageId, undefined, !openDetail.flags.flagged);
        lastAction = record;
        openDetail = { ...openDetail, flags: { ...openDetail.flags, flagged: !openDetail.flags.flagged } };
        await refreshFolderList(folderId);
      } else if (kind === "archive") {
        const record = await archiveMessage(folderId, messageId);
        lastAction = record;
        clearOpen();
        await refreshFolderList(folderId);
      } else if (kind === "delete") {
        const record = await deleteMessage(folderId, messageId);
        lastAction = record;
        clearOpen();
        await refreshFolderList(folderId);
      }
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function onUndo() {
    if (!lastAction) return;
    const id = lastAction.id;
    const folderId = lastAction.folderId;
    lastAction = null;
    try {
      await undoAction(id);
      await refreshFolderList(folderId);
      await reloadOpenDetail();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }
</script>

<div class="shell">
  {#if statusText}
    <div
      class="conn-bar"
      class:online={connection?.kind === "online"}
      class:offline={connection?.kind === "offline"}
      class:reconnecting={connection?.kind === "reconnecting"}
      role="status"
    >
      {statusText}
    </div>
  {/if}
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
      <MessageList
        {messages}
        total={messageTotal}
        {syncProgress}
        selectedId={selectedMessageId}
        onselect={onSelectMessage}
      />
    {:else if activeAccount}
      <p class="muted pad">{accountLabel(activeAccount)}</p>
    {/if}
    {#if error}
      <p class="status error pad">{error}</p>
    {/if}
  </section>

  <main class="pane reading" aria-label="Reading">
    <ReadingPane
      message={openDetail}
      loading={openLoading}
      error={openError}
      onaction={onMessageAction}
    />
  </main>

  <UndoBar action={lastAction} onundo={onUndo} ondismiss={() => (lastAction = null)} />
</div>

<style>
  .shell {
    display: grid;
    grid-template-columns: 260px 360px 1fr;
    grid-template-rows: auto 1fr auto;
    height: 100vh;
    width: 100vw;
    background: var(--bg);
  }

  .conn-bar {
    grid-column: 1 / -1;
    grid-row: 1;
    padding: 4px 12px;
    font-size: 11px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-elevated);
    color: var(--text-muted);
  }

  .conn-bar.online {
    color: #3fb950;
  }

  .conn-bar.offline {
    color: var(--text-muted);
  }

  .conn-bar.reconnecting {
    color: #d29922;
  }

  .pane {
    grid-row: 2;
  }

  :global(.undo-bar) {
    grid-column: 1 / -1;
    grid-row: 3;
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
    min-height: 0;
  }

  .pad {
    padding: 12px 14px;
  }

  .muted {
    color: var(--text-muted);
    font-size: 13px;
  }

  .status {
    font-size: 12px;
    word-break: break-all;
  }

  .error {
    color: #f85149;
  }
</style>
