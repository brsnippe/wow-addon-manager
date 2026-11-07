<script lang="ts">
  import { clientLabel } from "$lib/client-choice";
  import type { GameClient } from "$lib/types";

  interface Props {
    clients: GameClient[];
    selected: string | null;
    onselect: (folder: string) => void;
    disabled?: boolean;
  }

  let { clients, selected, onselect, disabled = false }: Props = $props();
</script>

<label class="picker">
  <span class="label">Game version</span>
  <select
    value={selected}
    {disabled}
    onchange={(event) => onselect(event.currentTarget.value)}
  >
    {#each clients as client (client.folder)}
      <option value={client.folder}>{clientLabel(client)}</option>
    {/each}
  </select>
</label>

<style>
  .picker {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .label {
    color: var(--text-muted);
    font-size: var(--font-sm);
  }

  select {
    min-width: 13rem;
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-raised);
    font-size: var(--font-md);
    cursor: pointer;
  }

  select:disabled {
    cursor: not-allowed;
    opacity: 0.6;
  }
</style>
