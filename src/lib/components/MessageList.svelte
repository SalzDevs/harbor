<script lang="ts">
  import {
    formatMessageDate,
    messageFrom,
    messageSubject,
  } from "$lib/messages";
  import { strings } from "$lib/strings";
  import type { FolderSyncProgress, MessageListItem } from "$lib/types";

  type Props = {
    messages: MessageListItem[];
    total: number;
    syncProgress: FolderSyncProgress | null;
    emptyLabel?: string;
  };

  let {
    messages,
    total,
    syncProgress,
    emptyLabel = strings.noMessages,
  }: Props = $props();

  const ROW = 56;
  const OVERSCAN = 10;

  let scrollTop = $state(0);
  let viewportH = $state(400);

  const totalSize = $derived(messages.length * ROW);
  const startIndex = $derived(Math.max(0, Math.floor(scrollTop / ROW) - OVERSCAN));
  const endIndex = $derived(
    Math.min(
      messages.length,
      Math.ceil((scrollTop + viewportH) / ROW) + OVERSCAN,
    ),
  );
  const visible = $derived(messages.slice(startIndex, endIndex));
  const offsetY = $derived(startIndex * ROW);

  function onScroll(e: Event) {
    const el = e.currentTarget as HTMLDivElement;
    scrollTop = el.scrollTop;
    viewportH = el.clientHeight;
  }

  function onMountEl(node: HTMLDivElement) {
    viewportH = node.clientHeight;
    const ro = new ResizeObserver(() => {
      viewportH = node.clientHeight;
    });
    ro.observe(node);
    return {
      destroy() {
        ro.disconnect();
      },
    };
  }
</script>

<div class="list-wrap">
  {#if syncProgress && syncProgress.total > 0}
    <div class="progress" role="status">
      {strings.syncingMessages}
      {syncProgress.fetched} of ~{syncProgress.total}
    </div>
  {:else if syncProgress}
    <div class="progress" role="status">{strings.syncingMessages}</div>
  {/if}

  <div class="meta">
    {total}
    {strings.messages}
  </div>

  {#if messages.length === 0}
    <p class="muted empty">{emptyLabel}</p>
  {:else}
    <div class="scroller" use:onMountEl onscroll={onScroll}>
      <div class="virtual" style="height: {totalSize}px;">
        <div class="window" style="transform: translateY({offsetY}px);">
          {#each visible as msg (msg.id + ":" + msg.uid)}
            <div class="row" class:unread={!msg.flags.seen}>
              <div class="from">{messageFrom(msg)}</div>
              <div class="subject">
                {#if msg.flags.flagged}<span class="flag">★</span>{/if}
                {messageSubject(msg)}
              </div>
              <div class="date">{formatMessageDate(msg.dateUnix)}</div>
            </div>
          {/each}
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  .list-wrap {
    display: flex;
    flex-direction: column;
    min-height: 0;
    flex: 1;
  }

  .progress {
    padding: 8px 14px;
    font-size: 12px;
    color: var(--accent);
    border-bottom: 1px solid var(--border);
    background: var(--bg-elevated);
  }

  .meta {
    padding: 6px 14px;
    font-size: 11px;
    color: var(--text-muted);
    border-bottom: 1px solid var(--border);
  }

  .scroller {
    flex: 1;
    overflow: auto;
    position: relative;
  }

  .virtual {
    width: 100%;
    position: relative;
  }

  .window {
    will-change: transform;
  }

  .row {
    height: 56px;
    padding: 8px 14px;
    border-bottom: 1px solid var(--border);
    display: grid;
    grid-template-columns: 1fr auto;
    grid-template-rows: auto auto;
    column-gap: 12px;
    row-gap: 2px;
    box-sizing: border-box;
  }

  .row:hover {
    background: var(--bg-elevated);
  }

  .row.unread .from,
  .row.unread .subject {
    font-weight: 600;
    color: var(--text);
  }

  .from {
    grid-column: 1;
    font-size: 13px;
    color: var(--text);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .date {
    grid-column: 2;
    grid-row: 1;
    font-size: 11px;
    color: var(--text-muted);
    white-space: nowrap;
  }

  .subject {
    grid-column: 1 / -1;
    font-size: 12px;
    color: var(--text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .flag {
    color: #e3b341;
    margin-right: 4px;
  }

  .muted.empty {
    padding: 16px 14px;
    color: var(--text-muted);
    font-size: 13px;
  }
</style>
