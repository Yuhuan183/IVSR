export function bytes(n: number): string {
  const units = ["B", "KB", "MB", "GB"];
  let v = n;
  let u = 0;
  while (v >= 1024 && u < units.length - 1) {
    v /= 1024;
    u++;
  }
  return u === 0 ? `${n} B` : `${v.toFixed(1)} ${units[u]}`;
}

export function duration(ms: number): string {
  const s = ms / 1000;
  if (s < 60) return `${s.toFixed(1)}s`;
  const m = Math.floor(s / 60);
  return `${m}m${String(Math.floor(s % 60)).padStart(2, "0")}s`;
}

export function scaleText(scale: number): string {
  return Number.isInteger(scale) ? String(scale) : scale.toFixed(2).replace(/0+$/, "").replace(/\.$/, "");
}

export function basename(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}
