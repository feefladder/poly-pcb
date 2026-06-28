import { ref } from "vue";
import { Interface } from "./pkg/plurtfm";

export async function loadAsset(path: string) {
  const url = new URL(`./assets/${path}`, import.meta.url);
  const r = await fetch(url);

  if (r.status === 404) return null;
  const type = r.headers.get("content-type");

  if (type?.startsWith("text/html")) return null;

  if (!r.ok) return null;

  return new Uint8Array(await r.arrayBuffer());
}

export class PcbLoader {
  iface;
  loaded = new Set<string>();
  loading = new Map<string, Promise<void>>();

  busy = false;
  loadingCount = 0;

  constructor(iface: Interface) {
    this.iface = iface;
  }

  requestMany(missingVariants: number[][]) {
    const promises = [];

    for (const [nGon, variants] of missingVariants.entries()) {
      for (const variant of variants) {
        const key = `${nGon}-${variant}`;

        if (this.loaded.has(key)) continue;

        if (this.loading.has(key)) {
          promises.push(this.loading.get(key)!);
          continue;
        }

        const promise = this.loadOne(nGon, variant);
        this.loading.set(key, promise);
        promises.push(promise);
      }
    }

    return Promise.all(promises);
  }

  async loadOne(nGon: number, variant: number) {
    console.log(this);
    console.log(this.busy);
    this.busy = true;
    this.loadingCount++;

    try {
      const pcb = await loadAsset(
        `${nGon}-${variant.toString(2).padStart(2, "0")}.stl`,
      );

      if (pcb === null) {
        console.log(`No STL for ${nGon}-${variant}`);
        return;
      }

      this.iface.add_pcb(nGon, variant, pcb);
      this.loaded.add(`${nGon}-${variant}`);
    } finally {
      const key = `${nGon}-${variant}`;
      this.loading.delete(key);

      this.loadingCount--;
      this.busy = this.loading.size > 0;
    }
  }
}
