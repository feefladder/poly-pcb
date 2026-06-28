<script setup lang="ts">
import { ref, onMounted, watch, Ref } from "vue";
import { Interface } from "./pkg/plurtfm.js";
import { loadAsset, PcbLoader } from "./pcb_loader.js";

const polyhedra: Ref<string[]> = ref([]);
const pcbLoader = ref<PcbLoader | null>(null);
const canvas = ref(null);
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
    iface.set_polyhedron("tetrahedron");
    iface.on_resize();
    iface.render();

    pcbLoader.value!.requestMany([[], [], [], [0]]);
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
            <!-- <p v-if="pcbLoader.busy">
                Loading PCBs ({{ pcbLoader.loadingCount }})...
            </p> -->
        </aside>

        <main class="viewport-container">
            <canvas
                ref="canvas"
                tabindex="0"
                @keydown="iface.on_key"
                @click="iface.on_click"
                @next_polyhedron="selected = $event.detail"
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
