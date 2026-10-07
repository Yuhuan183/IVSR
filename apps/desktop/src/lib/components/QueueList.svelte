<script lang="ts">
  import { queue, type QueueItem } from "../stores/queue.svelte";
  import QueueRow from "./QueueRow.svelte";

  let { onview }: { onview: (item: QueueItem) => void } = $props();
</script>

<ul class="list">
  {#each queue.items as item (item.path)}
    <QueueRow {item} {onview} />
  {/each}
</ul>

<style>
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
</style>
