<script lang="ts">
  // Lazily requests a cached thumbnail from the core once the tile is visible.
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { ipc } from "../ipc";
  import type { MediaKind } from "../types";
  import Icon from "./Icon.svelte";

  let { path, kind }: { path: string | null; kind: MediaKind } = $props();

  let el: HTMLDivElement | undefined = $state();
  let src = $state<string | null>(null);

  $effect(() => {
    if (!el || !path || kind !== "image") return;
    const target = path;
    let cancelled = false;
    const io = new IntersectionObserver((entries) => {
      if (!entries.some((e) => e.isIntersecting)) return;
      io.disconnect();
      ipc
        .thumbnail(target)
        .then((thumb) => {
          if (!cancelled) src = convertFileSrc(thumb);
        })
        .catch(() => {});
    });
    io.observe(el);
    return () => {
      cancelled = true;
      io.disconnect();
    };
  });
</script>

<div class="thumb" bind:this={el}>
  {#if src}
    <img {src} alt="" decoding="async" />
  {:else}
    <Icon name={kind === "video" ? "film" : "image"} size={26} />
  {/if}
</div>

<style>
  .thumb {
    width: 100%;
    height: 100%;
    display: grid;
    place-items: center;
    color: var(--faint);
    overflow: hidden;
  }
  img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
</style>
