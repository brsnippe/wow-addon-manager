<script lang="ts">
  interface Props {
    value: string;
  }

  let { value = $bindable() }: Props = $props();
  let input: HTMLInputElement;

  export function focus() {
    input.focus();
    input.select();
  }

  function onkeydown(event: KeyboardEvent) {
    if (event.key === "Escape" && value !== "") {
      event.preventDefault();
      value = "";
    }
  }
</script>

<input
  bind:this={input}
  bind:value
  {onkeydown}
  type="search"
  aria-label="Search addons"
  placeholder="Search name, author or folder"
  autocomplete="off"
  spellcheck="false"
/>

<style>
  input {
    width: 16rem;
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
</style>
