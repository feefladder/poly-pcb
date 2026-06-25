<script setup>
import { ref, onMounted } from "vue";

const polyhedra = ref([]);
const canvas = ref(null);

let iface;

onMounted(async () => {
    const wasm = await import("./pkg/plurtfm.js");

    console.log(canvas.value);
    await wasm.default();

    iface = await wasm.init_iface(canvas.value);

    polyhedra.value = iface.polyhedron_names();
});
</script>

<template>
    <div class="layout">
        <aside class="sidebar">
            <select>
                <option v-for="name in polyhedra" :key="name">
                    {{ name }}
                </option>
            </select>
        </aside>

        <main class="viewport-container">
            <canvas ref="canvas"></canvas>
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

#viewport {
    width: 100%;
    height: 100%;
    display: block;
}
</style>
