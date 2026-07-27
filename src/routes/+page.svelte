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
  import ConversationList from "$lib/components/ConversationList.svelte";
  import ReadingPane from "$lib/components/ReadingPane.svelte";
  import SearchResults from "$lib/components/SearchResults.svelte";
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
    getViewMode,
    listConversations,
    listMessages,
    listThreadMessages,
    openMessage,
    searchMessages,
    setViewMode,
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
    ConversationListItem,
    Folder,
    FolderMailUpdated,
    FolderSyncProgress,
    MessageDetail,
    MessageListItem,
    Provider,
    SearchPage,
    SearchResult,
    ViewMode,
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
  let viewMode = $state<ViewMode>("conversation");
  let conversations = $state<ConversationListItem[]>([]);
  let conversationTotal = $state(0);
  let selectedThreadRoot = $state<string | null>(null);
  let threadMessages = $state<MessageListItem[]>([]);
  let searchQuery = $state("");
  let searchResults = $state<SearchResult[]>([]);
  let searchTotal = $state(0);
  let searching = $state(false);
  let searchActive = $state(false);
  let searchToken = 0;
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
          await loadListData(event.payload.folderId);
        }
      },
    );
    try {
      connection = await getConnectionStatus();
    } catch {
      /* ignore */
    }
    try {
      viewMode = await getViewMode();
    } catch {
      /* default conversation */
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
    selectedThreadRoot = null;
    threadMessages = [];
  }

  async function selectFolder(folderId: string) {
    activeFolderId = folderId;
    syncProgress = null;
    clearOpen();
    await loadListData(folderId);
    // Background header sync.
    const token = ++headerSyncToken;
    void (async () => {
      try {
        await syncFolderHeaders(folderId);
        if (token !== headerSyncToken || activeFolderId !== folderId) return;
        await loadListData(folderId);
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

  async function loadListData(folderId: string) {
    try {
      if (viewMode === "conversation") {
        const page = await listConversations(folderId, 300, 0);
        conversations = page.conversations;
        conversationTotal = page.total;
        messages = [];
        messageTotal = 0;
      } else {
        const page = await listMessages(folderId, 300, 0);
        messages = page.messages;
        messageTotal = page.total;
        conversations = [];
        conversationTotal = 0;
      }
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
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
      await loadListData(folderId);
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

  async function onToggleViewMode() {
    const next: ViewMode = viewMode === "conversation" ? "flat" : "conversation";
    viewMode = next;
    try {
      await setViewMode(next);
    } catch {
      /* ignore */
    }
    if (activeFolderId) {
      clearOpen();
      await loadListData(activeFolderId);
    }
  }

  async function onSelectConversation(conv: ConversationListItem) {
    if (!activeFolderId) return;
    selectedThreadRoot = conv.threadRoot;
    openLoading = true;
    openError = null;
    openDetail = null;
    const token = ++openToken;
    try {
      const msgs = await listThreadMessages(activeFolderId, conv.threadRoot);
      if (token !== openToken) return;
      threadMessages = msgs;
      if (msgs.length > 0) {
        const latest = msgs[msgs.length - 1];
        selectedMessageId = latest.id;
        openDetail = await openMessage(activeFolderId, latest.id);
      }
    } catch (e) {
      if (token !== openToken) return;
      openError = e instanceof Error ? e.message : String(e);
    } finally {
      if (token === openToken) openLoading = false;
    }
  }

  async function onSearch() {
    const q = searchQuery.trim();
    if (!activeId) return;
    if (q.length === 0) {
      searchActive = false;
      searchResults = [];
      searchTotal = 0;
      return;
    }
    searchActive = true;
    searching = true;
    const token = ++searchToken;
    try {
      const page = await searchMessages(activeId, q);
      if (token !== searchToken) return;
      searchResults = page.results;
      searchTotal = page.total;
    } catch (e) {
      if (token !== searchToken) return;
      error = e instanceof Error ? e.message : String(e);
    } finally {
      if (token === searchToken) searching = false;
    }
  }

  function onClearSearch() {
    searchQuery = "";
    searchActive = false;
    searchResults = [];
    searchTotal = 0;
  }

  async function onSelectSearchResult(result: SearchResult) {
    if (!activeFolderId) return;
    selectedMessageId = result.message.id;
    openLoading = true;
    openError = null;
    openDetail = null;
    const token = ++openToken;
    try {
      openDetail = await openMessage(result.message.folderId, result.message.id);
      if (token !== openToken) return;
    } catch (e) {
      if (token !== openToken) return;
      openError = e instanceof Error ? e.message : String(e);
    } finally {
      if (token === openToken) openLoading = false;
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
    <div class="pane-label row">
      <span>
        {#if searchActive}
          {strings.search}
        {:else if activeFolder}
          {folderLabel(activeFolder)}
        {:else}
          {strings.selectFolder}
        {/if}
      </span>
      {#if activeFolder && !searchActive}
        <button
          type="button"
          class="linkish"
          onclick={onToggleViewMode}
          title={viewMode === "conversation" ? strings.viewFlat : strings.viewConversation}
        >
          {viewMode === "conversation" ? strings.viewFlat : strings.viewConversation}
        </button>
      {/if}
    </div>
    {#if activeId}
      <div class="search-bar">
        <input
          type="text"
          class="search-input"
          placeholder={strings.searchPlaceholder}
          bind:value={searchQuery}
          oninput={onSearch}
        />
        {#if searchQuery.length > 0}
          <button type="button" class="clear" onclick={onClearSearch}>✕</button>
        {/if}
      </div>
    {/if}
    {#if searchActive}
      <SearchResults
        results={searchResults}
        total={searchTotal}
        query={searchQuery}
        loading={searching}
        selectedId={selectedMessageId}
        onselect={onSelectSearchResult}
      />
    {:else if activeFolder}
      {#if viewMode === "conversation"}
        <ConversationList
          conversations={conversations}
          total={conversationTotal}
          selectedThreadRoot={selectedThreadRoot}
          onselect={onSelectConversation}
        />
      {:else}
        <MessageList
          {messages}
          total={messageTotal}
          {syncProgress}
          selectedId={selectedMessageId}
          onselect={onSelectMessage}
        />
      {/if}
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

  .search-bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-elevated);
    flex-shrink: 0;
  }

  .search-input {
    flex: 1;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg-pane);
    color: var(--text);
    padding: 6px 10px;
    font-size: 13px;
    outline: none;
  }

  .search-input:focus {
    border-color: var(--accent);
  }

  .search-input::placeholder {
    color: var(--text-muted);
  }

  .clear {
    border: none;
    background: transparent;
    color: var(--text-muted);
    font-size: 14px;
    padding: 4px;
  }

  .clear:hover {
    color: var(--text);
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
