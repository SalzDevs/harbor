<script lang="ts">
  import {
    formatMessageDate,
    messageFrom,
    messageSubject,
  } from "$lib/messages";
  import { strings } from "$lib/strings";
  import type { SearchResult } from "$lib/types";

  type Props = {
    results: SearchResult[];
    total: number;
    query: string;
    loading: boolean;
    selectedId?: string | null;
    onselect?: (result: SearchResult) => void;
  };

  let {
    results,
    total,
    query,
    loading,
    selectedId = null,
    onselect,
  }: Props = $props();
</script>

<div class="search-wrap">
  {#if loading}
    <div class="meta">Searching…</div>
  {:else if query.trim().length === 0}
    <p class="muted empty">{strings.searchEmpty}</p>
  {:else if results.length === 0}
    <p class="muted empty">{strings.searchNoResults}</p>
  {:else}
    <div class="meta">{total} {total === 1 ? "match" : "matches"}</div>
    <div class="scroller">
      {#each results as result (result.message.id)}
        <button
          type="button"
          class="row"
          class:active={result.message.id === selectedId}
          onclick={() => onselect?.(result)}
        >
          <div class="from">{messageFrom(result.message)}</div>
          <div class="date">{formatMessageDate(result.message.dateUnix)}</div>
          <div class="subject">{messageSubject(result.message)}</div>
          <div class="snippet">{@html result.snippet}</div>
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .search-wrap {
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
  }

  .row {
    width: 100%;
    padding: 10px 14px;
    border: none;
    border-bottom: 1px solid var(--border);
    background: transparent;
    display: grid;
    grid-template-columns: 1fr auto;
    grid-template-rows: auto auto auto;
    column-gap: 12px;
    row-gap: 2px;
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

  .snippet {
    grid-column: 1 / -1;
    font-size: 11px;
    color: var(--text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .snippet :global(«) {
    color: var(--accent);
    font-weight: 600;
  }

  .snippet :global(») {
    color: var(--accent);
    font-weight: 600;
  }

  .muted.empty {
    padding: 16px 14px;
    color: var(--text-muted);
    font-size: 13px;
  }
</style>
