import { ref } from "vue";
import { Interface } from "./pkg/poly_pcb.js";

const assets = {
  ...import.meta.glob("./assets/*.glb", {
    eager: true,
    query: "?url",
    import: "default",
  }),
  ...import.meta.glob("./assets/*.stl", {
    eager: true,
    query: "?url",
    import: "default",
  }),
} as Record<string, string>;

/// Load an asset. Can be anything really
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

  pcb_exists(n_gon: number, variant: number): boolean {
    const base = `${n_gon}-${variant.toString(2).padStart(2, "0")}`;
    const stl = `./assets/${base}.stl`;
    if (!(stl in assets || `./assets/${base}.glb` in assets)) {
      return false;
    } else {
      return true;
    }
  }

  requestMany(missingVariants: number[][]) {
    const promises = [];

    for (const [nGon, variants] of missingVariants.entries()) {
      if (!variants) {
        continue;
      }
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
    this.busy = true;
    this.loadingCount++;

    try {
      const base = `${nGon}-${variant.toString(2).padStart(2, "0")}`;

      let pcb;
      let name;
      if (`./assets/${base}.glb` in assets) {
        name = `${base}.glb`;
        console.log("loading GLB!", base);
        pcb = await loadAsset(name);
      } else if (`./assets/${base}.stl` in assets) {
        name = `${base}.stl`;
        pcb = await loadAsset(name);
      } else {
        console.log(`No STL for ${nGon}-${variant}`);
        return;
      }

      this.iface.add_pcb(nGon, variant, pcb!, name);
      this.loaded.add(`${nGon}-${variant}`);
    } finally {
      const key = `${nGon}-${variant}`;
      this.loading.delete(key);

      this.loadingCount--;
      this.busy = this.loading.size > 0;
    }
  }
}
