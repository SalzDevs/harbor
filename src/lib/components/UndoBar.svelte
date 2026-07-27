<script lang="ts">
  import { strings } from "$lib/strings";
  import type { ActionRecord } from "$lib/types";

  type Props = {
    action: ActionRecord | null;
    onundo?: () => void;
    ondismiss?: () => void;
  };

  let { action, onundo, ondismiss }: Props = $props();

  let timer: ReturnType<typeof setTimeout> | null = null;

  $effect(() => {
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
    if (action) {
      timer = setTimeout(() => {
        timer = null;
        ondismiss?.();
      }, 8000);
    }
  });
</script>

{#if action}
  <div class="undo-bar" role="status">
    <span class="label">{action.label}</span>
    <button type="button" class="undo" onclick={() => onundo?.()}>{strings.undo}</button>
  </div>
{/if}

<style>
  .undo-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 16px;
    background: var(--bg-elevated);
    border-top: 1px solid var(--border);
    color: var(--text);
    font-size: 13px;
  }

  .undo {
    border: 1px solid var(--border);
    background: transparent;
    color: var(--accent);
    border-radius: 6px;
    padding: 4px 12px;
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .undo:hover {
    border-color: var(--accent);
  }
</style>
