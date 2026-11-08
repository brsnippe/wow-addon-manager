<script lang="ts">
  import { onMount } from "svelte";
  import { listAddons, listClients, toCommandError, wowRoot } from "$lib/api";
  import { filterAddons } from "$lib/addon-filter";
  import { defaultSort, type Sort } from "$lib/addon-table";
  import { chooseClient, rememberClient, rememberedClient } from "$lib/client-choice";
  import AddonTable from "$lib/components/AddonTable.svelte";
  import ClientPicker from "$lib/components/ClientPicker.svelte";
  import SearchBox from "$lib/components/SearchBox.svelte";
  import type { CommandError, GameClient, InstalledAddon } from "$lib/types";

  type Screen =
    | { kind: "loading" }
    | { kind: "noRoot" }
    | { kind: "error"; error: CommandError }
    | { kind: "ready"; root: string; clients: GameClient[] };

  let screen = $state<Screen>({ kind: "loading" });
  let selected = $state<string | null>(null);
  let addons = $state<InstalledAddon[]>([]);
  let addonsLoading = $state(false);
  let addonsError = $state<CommandError | null>(null);
  let sort = $state<Sort>(defaultSort);
  let query = $state("");
  let searchBox = $state<SearchBox>();

  // Only the newest addon request may update the list, so a slow response for a client the
  // user already switched away from can't overwrite the current one.
  let latestRequest = 0;

  const clients = $derived(screen.kind === "ready" ? screen.clients : []);
  const client = $derived(clients.find((c) => c.folder === selected) ?? null);
  const visible = $derived(filterAddons(addons, query));
  const countText = $derived.by(() => {
    const noun = addons.length === 1 ? "addon" : "addons";
    return visible.length === addons.length
      ? `${addons.length} ${noun}`
      : `${visible.length} of ${addons.length} ${noun}`;
  });

  onMount(start);

  async function start() {
    screen = { kind: "loading" };
    try {
      const root = await wowRoot();
      if (!root) {
        screen = { kind: "noRoot" };
        return;
      }
      await refresh(root.path, rememberedClient());
    } catch (error) {
      screen = { kind: "error", error: toCommandError(error) };
    }
  }

  /** Re-reads the clients and the selected client's addons, keeping the selection if it still exists. */
  async function refresh(root: string, preferred: string | null = selected) {
    try {
      const found = await listClients();
      screen = { kind: "ready", root, clients: found };
      const folder = chooseClient(found, preferred);
      if (folder !== selected) addons = [];
      selected = folder;
      if (folder) await loadAddons(folder);
    } catch (error) {
      screen = { kind: "error", error: toCommandError(error) };
    }
  }

  async function loadAddons(folder: string) {
    const request = ++latestRequest;
    addonsLoading = true;
    addonsError = null;
    try {
      const result = await listAddons(folder);
      if (request === latestRequest) addons = result;
    } catch (error) {
      if (request === latestRequest) addonsError = toCommandError(error);
    } finally {
      if (request === latestRequest) addonsLoading = false;
    }
  }

  function selectClient(folder: string) {
    selected = folder;
    addons = [];
    rememberClient(folder);
    loadAddons(folder);
  }

  function onRefresh() {
    if (screen.kind === "ready") refresh(screen.root);
  }

  function clearSearch() {
    query = "";
    searchBox?.focus();
  }

  function onWindowKeydown(event: KeyboardEvent) {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "f" && searchBox) {
      event.preventDefault();
      searchBox.focus();
    }
  }
</script>

<svelte:window onkeydown={onWindowKeydown} />

<div class="app">
  <header class="toolbar">
    <h1>WoW Addon Manager</h1>
    {#if clients.length > 0}
      <ClientPicker {clients} {selected} onselect={selectClient} />
      {#if !addonsLoading && !addonsError}
        <span class="count">{countText}</span>
      {/if}
    {/if}
    <div class="actions">
      {#if clients.length > 0}
        <SearchBox bind:this={searchBox} bind:value={query} />
      {/if}
      <button
        type="button"
        class="button"
        onclick={onRefresh}
        disabled={screen.kind !== "ready" || addonsLoading}
      >
        {addonsLoading && addons.length > 0 ? "Refreshing…" : "Refresh"}
      </button>
      <button
        type="button"
        class="button primary"
        disabled
        title="Updating arrives together with update checks"
      >
        Update All
      </button>
    </div>
  </header>

  <main>
    {#if screen.kind === "loading"}
      <AddonTable addons={[]} clientInterface={null} loading bind:sort />
    {:else if screen.kind === "noRoot"}
      <section class="message">
        <h2>World of Warcraft folder not set</h2>
        <p>
          Set the <code>WAM_WOW_ROOT</code> environment variable, or <code>wowRoot</code> in
          <code>settings.json</code>, to your World of Warcraft folder, then retry.
        </p>
        <button type="button" class="button" onclick={start}>Retry</button>
      </section>
    {:else if screen.kind === "error"}
      <section class="message" role="alert">
        <h2>Couldn't read your World of Warcraft folder</h2>
        <p class="error">{screen.error.message}</p>
        <button type="button" class="button" onclick={start}>Retry</button>
      </section>
    {:else if clients.length === 0}
      <section class="message">
        <h2>No game versions found</h2>
        <p>No installed game versions were found in <code>{screen.root}</code>.</p>
        <button type="button" class="button" onclick={onRefresh}>Retry</button>
      </section>
    {:else if addonsError}
      <section class="message" role="alert">
        <h2>Couldn't load addons</h2>
        <p class="error">{addonsError.message}</p>
        <button type="button" class="button" onclick={onRefresh}>Retry</button>
      </section>
    {:else if !addonsLoading && addons.length === 0}
      <section class="message">
        <h2>No addons installed for {client?.displayName ?? "this game version"}</h2>
        {#if client}
          <p>Nothing found in <code class="path">{client.path}\Interface\AddOns</code></p>
        {/if}
      </section>
    {:else if addons.length > 0 && visible.length === 0}
      <section class="message">
        <h2>No addons match “{query.trim()}”</h2>
        <p>Search looks at addon names, authors and folder names.</p>
        <button type="button" class="button" onclick={clearSearch}>Clear search</button>
      </section>
    {:else}
      <AddonTable
        addons={visible}
        clientInterface={client?.interface ?? null}
        loading={addonsLoading}
        bind:sort
      />
    {/if}
  </main>
</div>

<style>
  .app {
    display: grid;
    grid-template-rows: auto 1fr;
    height: 100vh;
  }

  .toolbar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-4);
    padding: var(--space-3) var(--space-6);
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }

  h1 {
    margin: 0 var(--space-2) 0 0;
    font-size: 1rem;
    font-weight: 600;
  }

  .count {
    color: var(--text-muted);
    font-size: var(--font-sm);
  }

  .actions {
    display: flex;
    gap: var(--space-2);
    margin-left: auto;
  }

  main {
    display: flex;
    flex-direction: column;
    min-height: 0;
    padding: var(--space-4) var(--space-6) var(--space-6);
  }

  .message {
    max-width: 36rem;
    margin: 15vh auto 0;
    text-align: center;
  }

  .message h2 {
    margin: 0 0 var(--space-2);
    font-size: 1.125rem;
    font-weight: 600;
  }

  .message p {
    margin: 0 0 var(--space-4);
    color: var(--text-muted);
    overflow-wrap: anywhere;
  }

  .message .error {
    color: var(--error);
  }

  code {
    color: var(--text);
  }

  .path {
    display: block;
    margin-top: var(--space-1);
  }
</style>
