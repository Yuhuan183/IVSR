<script lang="ts">
  import type { Snippet } from "svelte";

  let { title, aside, children }: { title: string; aside?: Snippet; children: Snippet } = $props();
</script>

<section>
  <header>
    <h3>{title}</h3>
    {#if aside}{@render aside()}{/if}
  </header>
  <div class="content">{@render children()}</div>
</section>

<style>
  section {
    padding: 14px 16px;
    border-bottom: 1px solid var(--border);
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 10px;
    min-height: 22px;
  }
  h3 {
    margin: 0;
    font-size: 11px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--muted);
    font-weight: 600;
  }
  .content {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
</style>
