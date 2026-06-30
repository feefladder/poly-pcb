<script setup lang="ts">
import { ref, onMounted, watch, Ref } from "vue";
import { Interface, PcbId } from "./pkg/plurtfm.js";
import { loadAsset, PcbLoader } from "./pcb_loader.js";

const polyhedra: Ref<string[]> = ref([]);
const pcbLoader = ref<PcbLoader | null>(null);
const canvas = ref();
const selected = ref("");

let iface: Interface;

onMounted(async () => {
    const wasm = await import("./pkg/plurtfm.js");

    console.log(canvas.value);
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

    const hash = decodeURIComponent(window.location.hash.slice(1));
    const humanized = hash.toLowerCase().replace(/[-_ ]+/g, " ");
    if (polyhedra.value.includes(humanized)) {
        selected.value = humanized;
    } else {
        selected.value = "tetrahedron";
    }
});

watch(selected, async (name) => {
    if (iface) {
        let missing_variants: Array<Array<number>> = iface.set_polyhedron(name);
        console.log("missing variants", missing_variants);
        iface.render();
        pcbLoader.value!.requestMany(missing_variants);
    }
});
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
                    (e: CustomEvent<PcbId>) => {
                        console.log(
                            `pcb ${e.detail.n_gon}-${e.detail.variant} requested`,
                            e,
                        );
                        const n_gon = e.detail.n_gon;
                        const arr: number[][] = [];
                        arr[n_gon] = [e.detail.variant];
                        pcbLoader?.requestMany(arr);
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
