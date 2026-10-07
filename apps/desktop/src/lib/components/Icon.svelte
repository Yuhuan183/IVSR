<script lang="ts">
  // Inline stroke icons (no icon font or network requests).
  const paths: Record<string, string> = {
    plus: "M12 5v14M5 12h14",
    folder: "M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z",
    play: "M7 5v14l11-7z",
    stop: "M7 7h10v10H7z",
    x: "M6 6l12 12M18 6L6 18",
    check: "M5 12l5 5L20 7",
    alert: "M12 8v5M12 16.5v.5M10.3 4.2 2.6 18a2 2 0 0 0 1.7 3h15.4a2 2 0 0 0 1.7-3L13.7 4.2a2 2 0 0 0-3.4 0z",
    image: "M4 5h16v14H4zM4 16l5-5 4 4 3-3 4 4M15 9.5h.01",
    film: "M4 5h16v14H4zM8 5v14M16 5v14M4 9h4M4 15h4M16 9h4M16 15h4",
    reveal: "M14 4h6v6M20 4l-8 8M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5",
    compare: "M12 3v18M5 7l-3 5 3 5M19 7l3 5-3 5",
    download: "M12 4v11M7 10l5 5 5-5M5 20h14",
    sparkle: "M12 3l1.8 5.2L19 10l-5.2 1.8L12 17l-1.8-5.2L5 10l5.2-1.8z",
    refresh: "M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7",
    trash: "M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3",
    chevron: "M9 6l6 6-6 6",
    upload: "M12 16V4M7 9l5-5 5 5M5 20h14",
    layers: "M12 3l9 5-9 5-9-5zM3 13l9 5 9-5M3 17l9 5 9-5",
    left: "M15 6l-6 6 6 6",
    right: "M9 6l6 6-6 6",
    minus: "M5 12h14",
    fit: "M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5",
    split: "M4 5h16v14H4zM12 5v14",
    slider: "M4 5h16v14H4zM12 3v18M9 12l-2 0M15 12l2 0",
    fade: "M4 5h10v10H4zM10 9h10v10H10z",
    pause: "M8 5v14M16 5v14",
    cpu: "M7 7h10v10H7zM9 3v4M15 3v4M9 17v4M15 17v4M3 9h4M3 15h4M17 9h4M17 15h4",
    gauge: "M4 15a8 8 0 1 1 16 0M12 15l4-5",
    search: "M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14zM20 20l-4-4",
  };
  let { name, size = 16 }: { name: string; size?: number } = $props();
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="1.8"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"><path d={paths[name]} /></svg>
