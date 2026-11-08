<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import {
    githubRateLimit,
    githubTokenStored,
    removeGithubToken,
    saveGithubToken,
    toCommandError,
  } from "$lib/api";
  import { describeRateLimit, NEW_TOKEN_URL } from "$lib/github-token";
  import type { RateLimit } from "$lib/types";

  let dialog: HTMLDialogElement;
  let input: HTMLInputElement;
  let stored = $state(false);
  let limit = $state<RateLimit | null>(null);
  let loading = $state(false);
  let token = $state("");
  let busy = $state(false);
  let error = $state<string | null>(null);
  let notice = $state<string | null>(null);

  export function open() {
    token = "";
    error = null;
    notice = null;
    dialog.showModal();
    input.focus();
    load();
  }

  async function load() {
    loading = true;
    try {
      stored = await githubTokenStored();
      limit = await githubRateLimit();
    } catch (e) {
      limit = null;
      error = toCommandError(e).message;
    } finally {
      loading = false;
    }
  }

  async function save(event: SubmitEvent) {
    event.preventDefault();
    busy = true;
    error = null;
    notice = null;
    try {
      limit = await saveGithubToken(token);
      stored = true;
      token = "";
      notice = "Token saved.";
    } catch (e) {
      error = toCommandError(e).message;
      input.focus();
    } finally {
      busy = false;
    }
  }

  async function remove() {
    busy = true;
    error = null;
    notice = null;
    try {
      await removeGithubToken();
      stored = false;
      notice = "Token removed.";
      limit = await githubRateLimit();
    } catch (e) {
      error = toCommandError(e).message;
    } finally {
      busy = false;
    }
  }

  function openTokenPage(event: MouseEvent) {
    event.preventDefault();
    openUrl(NEW_TOKEN_URL).catch((e) => (error = toCommandError(e).message));
  }
</script>

<dialog bind:this={dialog} aria-labelledby="token-title">
  <form onsubmit={save}>
    <h2 id="token-title">GitHub token</h2>
    <p>
      Without a token, GitHub allows 60 requests an hour, which a few refreshes can use up. A token
      raises this to 5,000. <a href={NEW_TOKEN_URL} onclick={openTokenPage}>Create a fine-grained
        token</a> on GitHub; its default access, public repositories read-only, is all the app needs.
    </p>

    <div class="status" aria-live="polite">
      <p>
        {stored ? "A token is stored in Windows Credential Manager." : "No token is stored."}
      </p>
      <p class="muted">
        {#if loading}
          Checking GitHub's rate limit…
        {:else if limit}
          {describeRateLimit(limit)}
        {/if}
      </p>
    </div>

    <label for="github-token">{stored ? "Replace the token" : "Token"}</label>
    <input
      bind:this={input}
      bind:value={token}
      id="github-token"
      type="password"
      placeholder="github_pat_…"
      autocomplete="off"
      spellcheck="false"
    />

    {#if error}
      <p class="error" role="alert">{error}</p>
    {:else if notice}
      <p class="notice" role="status">{notice}</p>
    {/if}

    <div class="buttons">
      {#if stored}
        <button type="button" class="button remove" onclick={remove} disabled={busy}>
          Remove token
        </button>
      {/if}
      <button type="button" class="button" onclick={() => dialog.close()}>Close</button>
      <button type="submit" class="button primary" disabled={busy || token.trim() === ""}>
        {busy ? "Checking…" : "Save"}
      </button>
    </div>
  </form>
</dialog>

<style>
  dialog {
    width: min(32rem, calc(100vw - 2 * var(--space-6)));
    padding: var(--space-6);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    color: var(--text);
  }

  dialog::backdrop {
    background: rgb(0 0 0 / 0.6);
  }

  h2 {
    margin: 0 0 var(--space-3);
    font-size: 1.125rem;
    font-weight: 600;
  }

  p {
    margin: 0 0 var(--space-3);
    font-size: var(--font-md);
  }

  a {
    color: var(--accent);
  }

  .status {
    margin-bottom: var(--space-4);
    padding: var(--space-3);
    border-radius: var(--radius);
    background: var(--surface-raised);
  }

  .status p {
    margin: 0;
  }

  .muted {
    min-height: 1.5em;
    color: var(--text-muted);
  }

  label {
    display: block;
    margin-bottom: var(--space-1);
    font-size: var(--font-sm);
    color: var(--text-muted);
  }

  input {
    width: 100%;
    margin-bottom: var(--space-3);
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-raised);
    font: inherit;
    font-size: var(--font-md);
    color: inherit;
  }

  input::placeholder {
    color: var(--text-muted);
  }

  .error {
    color: var(--error);
  }

  .notice {
    color: var(--text-muted);
  }

  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-4);
  }

  .remove {
    margin-right: auto;
  }
</style>
