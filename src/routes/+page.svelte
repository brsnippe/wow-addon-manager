<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  type WowRootSource = "env" | "settings" | "devSandbox";

  interface ResolvedWowRoot {
    path: string;
    source: WowRootSource;
  }

  const sourceLabels: Record<WowRootSource, string> = {
    env: "from WAM_WOW_ROOT",
    settings: "from settings",
    devSandbox: "development sandbox",
  };

  const wowRoot = invoke<ResolvedWowRoot | null>("wow_root");
</script>

<main class="container">
  <h1>WoW Addon Manager</h1>
  {#await wowRoot}
    <p>Looking for World of Warcraft…</p>
  {:then root}
    {#if root}
      <p>WoW folder: <code>{root.path}</code> ({sourceLabels[root.source]})</p>
    {:else}
      <p>No World of Warcraft folder configured.</p>
    {/if}
  {:catch error}
    <p class="error">{error}</p>
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

  .error {
    color: #ff6b6b;
  }
</style>
