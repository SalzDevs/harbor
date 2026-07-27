<script lang="ts">
  import { searchContacts, sendMail } from "$lib/compose";
  import { strings } from "$lib/strings";
  import type { Contact, MessageDetail } from "$lib/types";

  type Props = {
    accountId: string;
    fromEmail: string;
    fromName: string | null;
    signature?: string | null;
    replyTo?: MessageDetail | null;
    replyAll?: boolean;
    forward?: boolean;
    onclose?: () => void;
    onsent?: () => void;
  };

  let {
    accountId,
    fromEmail,
    fromName,
    signature = null,
    replyTo = null,
    replyAll = false,
    forward = false,
    onclose,
    onsent,
  }: Props = $props();

  let to = $state("");
  let cc = $state("");
  let bcc = $state("");
  let subject = $state("");
  let body = $state("");
  let sending = $state(false);
  let error = $state<string | null>(null);
  let contacts = $state<Contact[]>([]);

  // Initialize fields from reply/forward context.
  $effect(() => {
    if (replyTo) {
      if (forward) {
        to = "";
        subject = replyTo.subject.startsWith("Fwd:") ? replyTo.subject : `Fwd: ${replyTo.subject}`;
        body = `\n\n---------- Forwarded message ----------\nFrom: ${replyTo.fromName ?? replyTo.fromAddress ?? ""}\nSubject: ${replyTo.subject}\n\n${replyTo.body.textPlain ?? ""}`;
      } else {
        to = replyTo.fromAddress ?? "";
        if (replyAll && replyTo.toList) {
          cc = replyTo.toList;
        }
        subject = replyTo.subject.startsWith("Re:") ? replyTo.subject : `Re: ${replyTo.subject}`;
        body = `\n\nOn ${new Date(replyTo.dateUnix * 1000).toLocaleString()}, ${replyTo.fromName ?? replyTo.fromAddress ?? ""} wrote:\n\n${(replyTo.body.textPlain ?? "").split("\n").map((l) => `> ${l}`).join("\n")}`;
      }
    }
    if (signature && !body.includes(signature)) {
      body = `\n-- \n${signature}\n${body}`;
    }
  });

  async function onToInput() {
    const parts = to.split(",");
    const last = parts[parts.length - 1].trim();
    if (last.length >= 2) {
      try {
        contacts = await searchContacts(last, 5);
      } catch {
        contacts = [];
      }
    } else {
      contacts = [];
    }
  }

  function pickContact(c: Contact) {
    const parts = to.split(",");
    parts[parts.length - 1] = ` ${c.name ? `${c.name} <${c.address}>` : c.address}`;
    to = parts.join(",").trim() + ", ";
    contacts = [];
  }

  async function onSend() {
    sending = true;
    error = null;
    try {
      await sendMail({
        accountId,
        to,
        cc,
        bcc,
        subject,
        bodyText: body,
        inReplyTo: replyTo?.rfcMessageId ?? null,
        references: replyTo?.rfcMessageId ?? null,
      });
      onsent?.();
      onclose?.();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      sending = false;
    }
  }
</script>

<div class="compose">
  <div class="compose-header">
    <span class="from-label">{fromName ? `${fromName} <${fromEmail}>` : fromEmail}</span>
    <button type="button" class="close" onclick={() => onclose?.()}>✕</button>
  </div>

  <div class="field">
    <label>{strings.to}</label>
    <input type="text" bind:value={to} oninput={onToInput} placeholder="recipients" />
    {#if contacts.length > 0}
      <div class="autocomplete">
        {#each contacts as c (c.address)}
          <button type="button" class="ac-item" onclick={() => pickContact(c)}>
            {c.name ? `${c.name} <${c.address}>` : c.address}
          </button>
        {/each}
      </div>
    {/if}
  </div>

  <div class="field">
    <label>Cc</label>
    <input type="text" bind:value={cc} />
  </div>

  <div class="field">
    <label>Bcc</label>
    <input type="text" bind:value={bcc} />
  </div>

  <div class="field">
    <label>Subject</label>
    <input type="text" bind:value={subject} />
  </div>

  <textarea bind:value={body} class="body" placeholder="Write your message…"></textarea>

  {#if error}
    <p class="error">{error}</p>
  {/if}

  <div class="actions">
    <button type="button" class="send-btn" disabled={sending || !to.trim()} onclick={onSend}>
      {sending ? "Sending…" : "Send"}
    </button>
  </div>
</div>

<style>
  .compose {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg-pane);
  }

  .compose-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 16px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-elevated);
    flex-shrink: 0;
  }

  .from-label {
    font-size: 12px;
    color: var(--text-muted);
  }

  .close {
    border: none;
    background: transparent;
    color: var(--text-muted);
    font-size: 16px;
    cursor: pointer;
  }

  .close:hover {
    color: var(--text);
  }

  .field {
    display: grid;
    grid-template-columns: 50px 1fr;
    align-items: center;
    padding: 4px 16px;
    border-bottom: 1px solid var(--border);
    position: relative;
    flex-shrink: 0;
  }

  .field label {
    font-size: 12px;
    color: var(--text-muted);
    text-transform: uppercase;
  }

  .field input {
    border: none;
    background: transparent;
    color: var(--text);
    font-size: 13px;
    padding: 6px 0;
    outline: none;
  }

  .autocomplete {
    position: absolute;
    top: 100%;
    left: 66px;
    right: 16px;
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: 6px;
    z-index: 10;
    max-height: 200px;
    overflow: auto;
  }

  .ac-item {
    width: 100%;
    border: none;
    background: transparent;
    color: var(--text);
    text-align: left;
    padding: 8px 12px;
    font-size: 13px;
    cursor: pointer;
  }

  .ac-item:hover {
    background: var(--bg-pane);
  }

  .body {
    flex: 1;
    border: none;
    background: transparent;
    color: var(--text);
    font: 14px/1.5 ui-monospace, SFMono-Regular, Menlo, monospace;
    padding: 16px;
    resize: none;
    outline: none;
    min-height: 0;
  }

  .error {
    padding: 8px 16px;
    color: #f85149;
    font-size: 12px;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    padding: 10px 16px;
    border-top: 1px solid var(--border);
    flex-shrink: 0;
  }

  .send-btn {
    border: 1px solid var(--accent);
    background: var(--accent);
    color: #fff;
    border-radius: 6px;
    padding: 8px 24px;
    font-size: 13px;
    font-weight: 600;
  }

  .send-btn:disabled {
    opacity: 0.5;
  }
</style>
