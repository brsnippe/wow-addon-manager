<script lang="ts">
  import { listAddons, listClients, wowRoot } from "$lib/api";
  import type { WowRootSource } from "$lib/types";

  const sourceLabels: Record<WowRootSource, string> = {
    env: "from WAM_WOW_ROOT",
    settings: "from settings",
    devSandbox: "development sandbox",
  };

  const root = wowRoot();
</script>

<main class="container">
  <h1>WoW Addon Manager</h1>
  {#await root}
    <p>Looking for World of Warcraft…</p>
  {:then root}
    {#if root}
      <p>WoW folder: <code>{root.path}</code> ({sourceLabels[root.source]})</p>
      {#await listClients() then clients}
        <ul class="clients">
          {#each clients as client (client.folder)}
            <li>
              {client.displayName}
              <span class="muted">{client.version ?? "unknown version"}</span>
              {#await listAddons(client.folder)}
                <span class="muted">…</span>
              {:then addons}
                <span>{addons.length} addons</span>
              {:catch error}
                <span class="error">{error.message}</span>
              {/await}
            </li>
          {/each}
        </ul>
      {:catch error}
        <p class="error">{error.message}</p>
      {/await}
    {:else}
      <p>No World of Warcraft folder configured.</p>
    {/if}
  {:catch error}
    <p class="error">{error.message}</p>
  {/await}
</main>

<style>
  :root {
    font-family: Inter, "Segoe UI", system-ui, sans-serif;
    font-size: 16px;
    line-height: 1.5;
    color: #e6e6e6;
    background-color: #1a1a1a;
    -webkit-font-smoothing: antialiased;
  }

  .container {
    margin: 0;
    padding-top: 20vh;
    text-align: center;
  }

  h1 {
    font-weight: 600;
  }

  p {
    color: #9a9a9a;
  }

  code {
    color: #e6e6e6;
  }

  .clients {
    display: inline-flex;
    flex-direction: column;
    gap: 0.25rem;
    padding: 0;
    list-style: none;
    text-align: left;
  }

  .muted {
    color: #9a9a9a;
  }

  .error {
    color: #ff6b6b;
  }
</style>
