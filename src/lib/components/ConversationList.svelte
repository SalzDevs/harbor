<script lang="ts">
  import {
    formatMessageDate,
    messageFrom,
    messageSubject,
  } from "$lib/messages";
  import { strings } from "$lib/strings";
  import type { ConversationListItem } from "$lib/types";

  type Props = {
    conversations: ConversationListItem[];
    total: number;
    selectedThreadRoot?: string | null;
    emptyLabel?: string;
    onselect?: (conv: ConversationListItem) => void;
  };

  let {
    conversations,
    total,
    selectedThreadRoot = null,
    emptyLabel = strings.noMessages,
    onselect,
  }: Props = $props();

  const ROW = 56;
  const OVERSCAN = 10;

  let scrollTop = $state(0);
  let viewportH = $state(400);

  const totalSize = $derived(conversations.length * ROW);
  const startIndex = $derived(Math.max(0, Math.floor(scrollTop / ROW) - OVERSCAN));
  const endIndex = $derived(
    Math.min(
      conversations.length,
      Math.ceil((scrollTop + viewportH) / ROW) + OVERSCAN,
    ),
  );
  const visible = $derived(conversations.slice(startIndex, endIndex));
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
  <div class="meta">
    {total}
    {strings.messages}
  </div>

  {#if conversations.length === 0}
    <p class="muted empty">{emptyLabel}</p>
  {:else}
    <div class="scroller" use:onMountEl onscroll={onScroll}>
      <div class="virtual" style="height: {totalSize}px;">
        <div class="window" style="transform: translateY({offsetY}px);">
          {#each visible as conv (conv.threadRoot)}
            <button
              type="button"
              class="row"
              class:unread={conv.unreadCount > 0}
              class:active={conv.threadRoot === selectedThreadRoot}
              onclick={() => onselect?.(conv)}
            >
              <div class="from">{messageFrom(conv.latest)}</div>
              <div class="date">{formatMessageDate(conv.latest.dateUnix)}</div>
              <div class="subject">
                {#if conv.latest.flags.flagged}<span class="flag">★</span>{/if}
                {messageSubject(conv.latest)}
                {#if conv.messageCount > 1}
                  <span class="count">({conv.messageCount})</span>
                {/if}
              </div>
            </button>
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
    width: 100%;
    height: 56px;
    padding: 8px 14px;
    border: none;
    border-bottom: 1px solid var(--border);
    border-radius: 0;
    background: transparent;
    display: grid;
    grid-template-columns: 1fr auto;
    grid-template-rows: auto auto;
    column-gap: 12px;
    row-gap: 2px;
    box-sizing: border-box;
    text-align: left;
    color: inherit;
    cursor: pointer;
  }

  .row:hover {
    background: var(--bg-elevated);
  }

  .row.active {
    background: var(--bg-elevated);
    box-shadow: inset 2px 0 0 var(--accent);
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

  .count {
    color: var(--text-muted);
    margin-left: 4px;
  }

  .muted.empty {
    padding: 16px 14px;
    color: var(--text-muted);
    font-size: 13px;
  }
</style>
