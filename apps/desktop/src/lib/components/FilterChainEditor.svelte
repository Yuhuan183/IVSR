<script lang="ts">
  // Ordered filter steps for one stage: switch, reorder, tune and add steps.
  // Every filter describes its own parameters, so new filters need no UI code.
  import { t, tx } from "../i18n/index.svelte";
  import { app } from "../stores/app.svelte";
  import type { FilterStage, FilterStep, ParamValue } from "../types";
  import Icon from "./Icon.svelte";
  import ParamField from "./ParamField.svelte";

  let {
    stage,
    steps,
    onchange,
    onreset,
    idPrefix = "",
  }: {
    stage: FilterStage;
    steps: FilterStep[];
    onchange: (steps: FilterStep[]) => void;
    /** Shown when the steps differ from the built-in order. */
    onreset?: () => void;
    idPrefix?: string;
  } = $props();

  let open = $state<Record<number, boolean>>({});

  const available = $derived(app.filters.filter((f) => f.info.stages.includes(stage)));

  function update(index: number, step: FilterStep) {
    onchange(steps.map((s, i) => (i === index ? step : s)));
  }

  function setParam(index: number, key: string, value: ParamValue) {
    const step = steps[index];
    update(index, { ...step, params: { ...step.params, [key]: value } });
  }

  function move(index: number, delta: number) {
    const next = [...steps];
    const [step] = next.splice(index, 1);
    next.splice(index + delta, 0, step);
    const flags = { ...open };
    [flags[index], flags[index + delta]] = [open[index + delta], open[index]];
    open = flags;
    onchange(next);
  }

  function remove(index: number) {
    open = {};
    onchange(steps.filter((_, i) => i !== index));
  }

  function add(id: string) {
    if (!id) return;
    onchange([...steps, { id, enabled: true, params: {} }]);
  }
</script>

<div class="chain">
  {#if steps.length === 0}
    <p class="faint">{t("filters.empty")}</p>
  {/if}
  <ol>
    {#each steps as step, i (i)}
      {@const filter = app.filter(step.id)}
      <li class:off={!step.enabled}>
        <div class="row">
          <span class="order">{i + 1}</span>
          <input
            type="checkbox"
            checked={step.enabled}
            aria-label={filter ? tx(filter.info.name) : step.id}
            onchange={(e) => update(i, { ...step, enabled: (e.currentTarget as HTMLInputElement).checked })}
          />
          <button class="name ghost" title={filter ? tx(filter.info.description) : step.id} onclick={() => (open = { ...open, [i]: !open[i] })}>
            <span>{filter ? tx(filter.info.name) : step.id}</span>
            {#if filter?.info.uses_reference && stage === "post"}<span class="ref" title={t("filters.uses_reference")}>ref</span>{/if}
          </button>
          <span class="tools">
            <button class="ghost icon" title={t("filters.params")} aria-expanded={!!open[i]} onclick={() => (open = { ...open, [i]: !open[i] })}>
              <Icon name="sliders" size={13} />
            </button>
            <button class="ghost icon" title={t("filters.move_up")} disabled={i === 0} onclick={() => move(i, -1)}><Icon name="up" size={13} /></button>
            <button class="ghost icon" title={t("filters.move_down")} disabled={i === steps.length - 1} onclick={() => move(i, 1)}>
              <Icon name="down" size={13} />
            </button>
            <button class="ghost icon" title={t("filters.remove")} onclick={() => remove(i)}><Icon name="x" size={13} /></button>
          </span>
        </div>
        {#if open[i] && filter}
          <div class="params">
            <p class="desc">{tx(filter.info.description)}</p>
            {#each filter.params as spec (spec.key)}
              <ParamField
                {spec}
                idPrefix="{idPrefix}{stage}-{i}-"
                value={step.params[spec.key]}
                onchange={(v) => setParam(i, spec.key, v)}
              />
            {/each}
          </div>
        {/if}
      </li>
    {/each}
  </ol>
  <div class="footer">
    <select
      aria-label={t("filters.add")}
      value=""
      onchange={(e) => {
        const el = e.currentTarget as HTMLSelectElement;
        add(el.value);
        el.value = "";
      }}
    >
      <option value="">+ {t("filters.add")}</option>
      {#each available as f (f.info.id)}<option value={f.info.id}>{tx(f.info.name)}</option>{/each}
    </select>
    {#if onreset}
      <button class="ghost small" title={t("filters.reset_hint")} onclick={onreset}>{t("filters.reset")}</button>
    {/if}
  </div>
</div>

<style>
  .chain {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  ol {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  li {
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--panel-2);
  }
  li.off .name,
  li.off .order {
    opacity: 0.5;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 3px 4px 3px 8px;
  }
  .order {
    width: 14px;
    font-size: 11px;
    color: var(--faint);
    font-variant-numeric: tabular-nums;
  }
  .name {
    flex: 1;
    min-width: 0;
    justify-content: flex-start;
    gap: 6px;
    padding: 3px 4px;
    border: none;
    font-weight: 500;
  }
  .name span:first-child {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ref {
    font-size: 10px;
    padding: 0 5px;
    border-radius: 99px;
    background: var(--panel-3);
    color: var(--accent-2);
  }
  .tools {
    display: flex;
  }
  .icon {
    padding: 4px;
  }
  .params {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 4px 10px 10px;
    border-top: 1px solid var(--border);
  }
  .desc {
    margin: 4px 0 0;
    font-size: 11.5px;
    color: var(--muted);
  }
  .footer {
    display: flex;
    gap: 6px;
  }
  .footer select {
    flex: 1;
  }
  .small {
    font-size: 11.5px;
  }
</style>
