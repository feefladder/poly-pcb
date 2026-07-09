<script setup lang="ts">
import { ref, onMounted, watch, type Ref } from "vue";
import { Interface, PcbId, VarId } from "./pkg/plurtfm.js";
import { loadAsset, PcbLoader } from "./pcb_loader.js";

if (import.meta.hot) {
    import.meta.hot.accept(() => {
        location.reload();
    });
}

// make match rust
type VariantMap = [number, number[]][];

const polyhedra: Ref<string[]> = ref([]);
const pcbLoader = ref<PcbLoader | null>(null);
const canvas = ref();
const selected = ref("tetrahedron");
const variant_map: Ref<VariantMap> = ref([[3, [4, 4, 2, 0, 3]]]);

let iface: Interface;

window.addEventListener("hashchange", () => {
    apply_url();
});

function update_url(name: string, map: VariantMap) {
    let hash = `#/${name.replace(/ /g, "-")}`;

    const params = new URLSearchParams();

    for (const [nGon, variants] of map) {
        params.set(
            nGon.toString(),
            variants.map((v) => v.toString(16)).join(""),
        );
    }

    const query = params.toString();
    if (query) {
        hash += `?${query}`;
    }

    history.replaceState(null, "", hash);
}

function apply_url() {
    const hash = decodeURIComponent(location.hash.slice(2)); // remove "#/"

    const [poly_url, query = ""] = hash.split("?", 2);
    const polyhedron = poly_url?.toLowerCase().replace(/[-_ ]+/g, " ");
    const entries: [number, number[]][] = [];

    for (const [nGon, encoded] of new URLSearchParams(query)) {
        entries.push([Number(nGon), [...encoded].map((c) => parseInt(c, 16))]);
    }

    if (entries.length > 0) {
        variant_map.value = entries;
    }

    if (
        polyhedron &&
        polyhedra.value.includes(polyhedron) &&
        selected.value != polyhedron
    ) {
        console.log(
            "setting polyhedron to ",
            polyhedron,
            " because ",
            selected.value,
            " is different ",
            polyhedron == selected.value,
            polyhedron === selected.value,
        );
        selected.value = polyhedron;
    } else {
        console.log("could not find ", polyhedron);
        selected.value = "tetrahedron";
    }
}

onMounted(async () => {
    const wasm = await import("./pkg/plurtfm.js");

    await wasm.default();
    let db = await loadAsset("polydb.sqlite3");
    iface = wasm.init_iface(canvas.value!, db!);
    pcbLoader.value = new PcbLoader(iface);

    polyhedra.value = iface.polyhedron_names();

    const ro = new ResizeObserver(() => {
        iface.on_resize();
        iface.render();
    });
    ro.observe(canvas.value);
    apply_url();
});

watch(
    [selected, variant_map],
    async ([new_name, new_map]) => {
        if (iface) {
            console.log("setting poly with variant map ", variant_map.value);
            let missing_variants: Array<Array<number>> = iface.set_polyhedron(
                new_name,
                new_map,
            );
            console.log("missing variants", missing_variants);
            pcbLoader.value!.requestMany(missing_variants);
        }
        update_url(new_name, new_map);
    },
    { deep: true },
);

function on_request_pcb(var_id: VarId) {
    console.log("request pcb", var_id);

    // check if there is actually an stl for the requested variant???? otherwise cycle to 0
    const { nth_ngon, pcb_id } = var_id;
    let { n_gon, variant } = pcb_id;

    console.log("requested pcb for ", n_gon, variant);
    if (!pcbLoader.value?.pcb_exists(n_gon, variant)) {
        console.warn(`pcb ${n_gon} version ${variant} does not exist`);
        variant = 0;
    }

    let entry = variant_map.value.find(([n]) => n === n_gon);

    if (!entry) {
        entry = [n_gon, []];
        variant_map.value.push(entry);
    }

    const variants = entry[1];

    while (variants.length <= nth_ngon) {
        variants.push(0);
    }

    variants[nth_ngon] = variant;
}
</script>

<template>
    <div class="layout">
        <aside class="sidebar">
            <select v-model="selected">
                <option v-for="name in polyhedra" :key="name">
                    {{ name }}
                </option>
            </select>
            <p v-if="pcbLoader?.busy">
                Loading PCBs ({{ pcbLoader.loadingCount }})...
            </p>
        </aside>

        <main class="viewport-container">
            <canvas
                ref="canvas"
                tabindex="0"
                @keydown="iface.on_key"
                @next_polyhedron="selected = $event.detail"
                @request_pcb="
                    (e: CustomEventInit<VarId>) => {
                        on_request_pcb(e.detail!);
                    }
                "
                @pointerdown="iface?.on_pointer_down"
                @pointermove="iface?.on_pointer_move"
                @pointerup="iface?.on_pointer_up"
                @wheel.prevent="iface?.on_wheel"
                @click="iface?.on_click"
                @dblclick="iface?.next_polyhedron"
            ></canvas>
        </main>
    </div>
</template>

<style>
.layout {
    display: flex;
    height: 100vh;
}

.sidebar {
    width: 250px;
}

.viewport-container {
    flex: 1;
}

.viewport-container canvas {
    width: 100%;
    height: 100%;
    display: block;
}
</style>
