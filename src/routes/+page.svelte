<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { strings } from "$lib/strings";
  import type { AppInfo } from "$lib/types";

  let info = $state<AppInfo | null>(null);
  let error = $state<string | null>(null);

  onMount(async () => {
    try {
      info = await invoke<AppInfo>("app_info");
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  });
</script>

<div class="shell">
  <aside class="pane folders" aria-label="Folders">
    <div class="pane-label">{strings.appTitle}</div>
  </aside>

  <section class="pane list" aria-label="Messages">
    <div class="pane-label">Inbox</div>
  </section>

  <main class="pane reading" aria-label="Reading">
    <div class="empty">
      <h1>{strings.emptyShellHeading}</h1>
      <p>{strings.emptyShellBody}</p>
      {#if error}
        <p class="status error">{error}</p>
      {:else if info}
        <p class="status">{strings.statusReady} · {info.core} · {info.db}</p>
      {:else}
        <p class="status">{strings.statusLoading}</p>
      {/if}
    </div>
  </main>
</div>

<style>
  .shell {
    display: grid;
    grid-template-columns: 220px 320px 1fr;
    height: 100vh;
    width: 100vw;
    background: var(--bg);
  }

  .pane {
    min-width: 0;
    min-height: 0;
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
  }

  .reading {
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .empty {
    text-align: center;
    padding: 32px;
    max-width: 360px;
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

  .status {
    margin-top: 20px !important;
    font-size: 12px;
    opacity: 0.9;
  }

  .error {
    color: #f85149;
  }
</style>
