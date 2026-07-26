<script lang="ts">
  import {
    formatMessageDate,
    messageFrom,
    messageSubject,
  } from "$lib/messages";
  import { strings } from "$lib/strings";
  import type { MessageDetail } from "$lib/types";

  type Props = {
    message: MessageDetail | null;
    loading?: boolean;
    error?: string | null;
  };

  let { message, loading = false, error = null }: Props = $props();

  let loadImages = $state(false);

  // Reset image opt-in when message changes.
  $effect(() => {
    if (message?.id) {
      loadImages = false;
    }
  });

  const htmlSrcdoc = $derived.by(() => {
    if (!message?.body.textHtmlSafe) return null;
    const csp = loadImages
      ? "default-src 'none'; img-src https: http: data: cid:; style-src 'unsafe-inline'; font-src data: https: http:; base-uri 'none'; form-action 'none'; script-src 'none'"
      : "default-src 'none'; img-src data: cid:; style-src 'unsafe-inline'; font-src data:; base-uri 'none'; form-action 'none'; script-src 'none'";
    const body = message.body.textHtmlSafe;
    return `<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="${csp}"><style>
html,body{margin:0;padding:0;background:#12161c;color:#e6edf3;font:14px/1.5 system-ui,sans-serif;word-wrap:break-word}
a{color:#3d8bfd}
img{max-width:100%;height:auto}
pre,code{white-space:pre-wrap;word-break:break-word}
</style></head><body>${body}</body></html>`;
  });
</script>

{#if loading}
  <div class="state">{strings.loadingMessage}</div>
{:else if error}
  <div class="state error">{error}</div>
{:else if !message}
  <div class="state empty">
    <h1>{strings.emptyShellHeading}</h1>
    <p>{strings.emptyShellBody}</p>
  </div>
{:else}
  <article class="reading">
    <header class="hdr">
      <h1>{messageSubject(message)}</h1>
      <div class="meta-row">
        <span class="k">{strings.from}</span>
        <span>{messageFrom(message)}{message.fromAddress ? ` <${message.fromAddress}>` : ""}</span>
      </div>
      {#if message.toList}
        <div class="meta-row">
          <span class="k">{strings.to}</span>
          <span>{message.toList}</span>
        </div>
      {/if}
      <div class="meta-row">
        <span class="k">{strings.date}</span>
        <span>{formatMessageDate(message.dateUnix)}</span>
      </div>
      {#if message.hasRemoteImages && !loadImages}
        <div class="images-bar">
          <span>{strings.imagesBlocked}</span>
          <button type="button" class="btn" onclick={() => (loadImages = true)}>
            {strings.loadImages}
          </button>
        </div>
      {/if}
    </header>

    {#if htmlSrcdoc}
      <iframe
        class="html-frame"
        title={messageSubject(message)}
        sandbox=""
        srcdoc={htmlSrcdoc}
      ></iframe>
    {:else if message.body.textPlain}
      <pre class="plain">{message.body.textPlain}</pre>
    {:else}
      <div class="state">(empty body)</div>
    {/if}
  </article>
{/if}

<style>
  .state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    padding: 32px;
    color: var(--text-muted);
    text-align: center;
  }

  .state.empty h1 {
    margin: 0 0 8px;
    font-size: 28px;
    font-weight: 600;
    color: var(--text);
  }

  .state.error {
    color: #f85149;
  }

  .reading {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }

  .hdr {
    padding: 16px 20px 12px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }

  .hdr h1 {
    margin: 0 0 12px;
    font-size: 18px;
    font-weight: 600;
    line-height: 1.3;
  }

  .meta-row {
    display: grid;
    grid-template-columns: 48px 1fr;
    gap: 8px;
    font-size: 13px;
    margin-bottom: 4px;
    color: var(--text);
  }

  .k {
    color: var(--text-muted);
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .images-bar {
    margin-top: 12px;
    display: flex;
    align-items: center;
    gap: 12px;
    font-size: 12px;
    color: var(--text-muted);
  }

  .btn {
    border: 1px solid var(--border);
    background: var(--bg-elevated);
    color: var(--text);
    border-radius: 6px;
    padding: 4px 10px;
    font-size: 12px;
  }

  .btn:hover {
    border-color: var(--accent);
  }

  .html-frame {
    flex: 1;
    width: 100%;
    border: 0;
    background: var(--bg-pane);
    min-height: 0;
  }

  .plain {
    flex: 1;
    margin: 0;
    padding: 16px 20px;
    overflow: auto;
    white-space: pre-wrap;
    word-break: break-word;
    font: 14px/1.5 ui-monospace, SFMono-Regular, Menlo, monospace;
    color: var(--text);
  }
</style>
