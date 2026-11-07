<script lang="ts">
  import {
    formatInterface,
    nextSort,
    sortAddons,
    targetInterface,
    type Sort,
    type SortKey,
  } from "$lib/addon-table";
  import type { InstalledAddon } from "$lib/types";

  interface Props {
    addons: InstalledAddon[];
    clientInterface: number | null;
    /** With no rows yet this shows placeholder rows; otherwise the current rows stay visible. */
    loading?: boolean;
    sort: Sort;
  }

  let { addons, clientInterface, loading = false, sort = $bindable() }: Props = $props();

  const columns: { key: SortKey; label: string }[] = [
    { key: "name", label: "Name" },
    { key: "status", label: "Status" },
    { key: "latestVersion", label: "Latest Version" },
    { key: "gameVersion", label: "Game Version" },
    { key: "authors", label: "Authors" },
  ];
  const skeletonRows = Array.from({ length: 10 }, (_, index) => index);
  const notChecked = "Update checks are not available yet";

  const rows = $derived(sortAddons(addons, sort, clientInterface));
  const showSkeleton = $derived(loading && addons.length === 0);

  function gameVersion(addon: InstalledAddon): string {
    const target = targetInterface(addon, clientInterface);
    return target === null ? "—" : formatInterface(target);
  }
</script>

<div class="scroll" aria-busy={loading}>
  <table>
    <colgroup>
      <col class="col-name" />
      <col class="col-status" />
      <col class="col-latest" />
      <col class="col-game" />
      <col class="col-authors" />
    </colgroup>
    <thead>
      <tr>
        {#each columns as column (column.key)}
          <th scope="col" aria-sort={sort.key === column.key ? sort.direction : undefined}>
            <button type="button" onclick={() => (sort = nextSort(sort, column.key))}>
              {column.label}
              <span class="arrow" aria-hidden="true">
                {#if sort.key === column.key}{sort.direction === "ascending" ? "▲" : "▼"}{/if}
              </span>
            </button>
          </th>
        {/each}
      </tr>
    </thead>
    <tbody>
      {#if showSkeleton}
        {#each skeletonRows as row (row)}
          <tr class="skeleton" aria-hidden="true">
            <td><span class="bar wide"></span><span class="bar short"></span></td>
            <td><span class="bar"></span></td>
            <td><span class="bar"></span></td>
            <td><span class="bar"></span></td>
            <td><span class="bar"></span></td>
          </tr>
        {/each}
      {:else}
        {#each rows as addon (addon.name)}
          <tr>
            <td>
              <div class="title" title={addon.notes ?? addon.title}>{addon.title}</div>
              <div class="sub">{addon.version ?? "Unknown version"}</div>
            </td>
            <td class="muted" title={notChecked}>Not checked</td>
            <td class="muted" title={notChecked}>—</td>
            <td>{gameVersion(addon)}</td>
            <td class="truncate" title={addon.author ?? undefined}>{addon.author ?? "—"}</td>
          </tr>
        {/each}
      {/if}
    </tbody>
  </table>
  {#if showSkeleton}
    <p class="visually-hidden" role="status">Loading addons…</p>
  {/if}
</div>

<style>
  .scroll {
    flex: 0 1 auto;
    min-height: 0;
    overflow: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
  }

  table {
    width: 100%;
    border-collapse: collapse;
    table-layout: fixed;
    font-size: var(--font-md);
  }

  .col-name {
    width: 38%;
  }
  .col-status {
    width: 13%;
  }
  .col-latest {
    width: 14%;
  }
  .col-game {
    width: 13%;
  }
  .col-authors {
    width: 22%;
  }

  th {
    position: sticky;
    top: 0;
    z-index: 1;
    padding: 0;
    border-bottom: 1px solid var(--border);
    background: var(--surface-raised);
    text-align: left;
  }

  th button {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    width: 100%;
    padding: var(--space-2) var(--space-3);
    border: 0;
    background: none;
    color: var(--text-muted);
    font-size: var(--font-sm);
    font-weight: 600;
    text-align: left;
    cursor: pointer;
  }

  th button:hover,
  th[aria-sort] button {
    color: var(--text);
  }

  th button:focus-visible {
    outline-offset: -2px;
  }

  .arrow {
    font-size: 0.625rem;
  }

  td {
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--border);
    vertical-align: middle;
  }

  tbody tr:last-child td {
    border-bottom: 0;
  }

  tbody tr:not(.skeleton):hover {
    background: var(--surface-hover);
  }

  .title,
  .sub,
  .truncate {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .title {
    font-weight: 500;
  }

  .sub,
  .muted {
    color: var(--text-muted);
  }

  .sub {
    font-size: var(--font-sm);
  }

  .bar {
    display: block;
    width: 60%;
    height: 0.625rem;
    border-radius: 3px;
    background: var(--surface-raised);
    animation: pulse 1.4s ease-in-out infinite;
  }

  .bar.wide {
    width: 70%;
    margin-bottom: var(--space-2);
  }

  .bar.short {
    width: 30%;
  }

  @keyframes pulse {
    50% {
      opacity: 0.5;
    }
  }
</style>
