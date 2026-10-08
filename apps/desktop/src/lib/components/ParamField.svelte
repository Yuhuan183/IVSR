<script lang="ts">
  // Renders one engine parameter from its schema; new engines need no UI code.
  import { tx } from "../i18n/index.svelte";
  import type { ParamSpec, ParamValue } from "../types";

  let {
    spec,
    value,
    onchange,
    idPrefix = "",
  }: { spec: ParamSpec; value: ParamValue | undefined; onchange: (v: ParamValue) => void; idPrefix?: string } = $props();

  const id = $derived(`${idPrefix}param-${spec.key}`);

  const current = $derived(value ?? spec.default);

  function number(e: Event, integer: boolean) {
    const raw = (e.currentTarget as HTMLInputElement).valueAsNumber;
    if (Number.isNaN(raw)) return;
    onchange(integer ? Math.round(raw) : raw);
  }
</script>

<div class="field" class:inline={spec.kind.type === "bool"} title={tx(spec.description)}>
  <label for={id}>{tx(spec.label)}</label>
  {#if spec.kind.type === "bool"}
    <input
      {id}
      type="checkbox"
      checked={current === true}
      onchange={(e) => onchange((e.currentTarget as HTMLInputElement).checked)}
    />
  {:else if spec.kind.type === "int" || spec.kind.type === "float"}
    <input
      {id}
      type="number"
      value={current}
      min={spec.kind.min ?? undefined}
      max={spec.kind.max ?? undefined}
      step={spec.kind.type === "int" ? 1 : 0.1}
      onchange={(e) => number(e, spec.kind.type === "int")}
    />
  {:else if spec.kind.type === "enum"}
    <select {id} value={String(current)} onchange={(e) => onchange((e.currentTarget as HTMLSelectElement).value)}>
      {#each spec.kind.options as option (option.value)}
        <option value={option.value}>{tx(option.label)}</option>
      {/each}
    </select>
  {:else}
    <input
      {id}
      type="text"
      value={String(current)}
      onchange={(e) => onchange((e.currentTarget as HTMLInputElement).value)}
    />
  {/if}
  <p class="hint">{tx(spec.description)}</p>
</div>

<style>
  .field {
    display: grid;
    grid-template-columns: 1fr 110px;
    align-items: center;
    gap: 2px 10px;
  }
  .field.inline {
    grid-template-columns: 1fr auto;
  }
  label {
    font-weight: 500;
  }
  .hint {
    grid-column: 1 / -1;
    margin: 0;
    color: var(--faint);
    font-size: 11.5px;
  }
</style>
