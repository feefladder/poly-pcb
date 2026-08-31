<script setup lang="ts">
import { ref, onMounted, watch, type Ref } from "vue";
import {
    CurrentStep,
    Interface,
    PcbId,
    VarId,
    type PcbDesign,
} from "./pkg/poly_pcb.js";
import { loadAsset, PcbLoader } from "./pcb_loader.js";

// hot reload triggers full page reload to fix double-init wasm
if (import.meta.hot) {
    import.meta.hot.accept(() => {
        location.reload();
    });
}

// make match rust
type VariantMap = [number, number[]][];
type Path = number[];

const polyhedra: Ref<string[]> = ref([]);
const pcbLoader = ref<PcbLoader | null>(null);
const canvas = ref();
const mode = ref<CurrentStep>(CurrentStep.SelectPoly);
const uiState = ref<PcbDesign>({
    polyhedron: "tetrahedron",
    variant_map: [[3, [4, 0, 1, 2]]],
    path: { start_ngon: 3, start_nth: 0, turns: [0, 0, 1, 1] },
});

const steps = Object.values(CurrentStep).filter(
    (step): step is CurrentStep => typeof step === "number",
);
let iface: Interface;

window.addEventListener("hashchange", () => {
    apply_url();
});

function update_url() {
    const name = uiState.value.polyhedron;
    const map = uiState.value.variant_map;
    const path = uiState.value.path;
    let hash = `#/${name.replace(/ /g, "-")}`;

    const params = new URLSearchParams();

    for (const [nGon, variants] of map) {
        params.set(
            nGon.toString(),
            variants.map((v) => v.toString(16)).join(""),
        );
    }
    if (path.turns) {
        params.set(
            "path",
            `${path.start_ngon}.${path.start_nth}-${path.turns.map((t) => t.toString(16)).join("")}`,
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

    const [polyUrl, query = ""] = hash.split("?", 2);
    const polyhedron = polyUrl?.toLowerCase().replace(/[-_ ]+/g, " ");
    const entries: [number, number[]][] = [];
    const params = new URLSearchParams(query);

    for (const [key, encoded] of params) {
        const nGon = Number(key);
        if (!Number.isInteger(nGon) || nGon < 3 || nGon > 10) continue;
        entries.push([Number(nGon), [...encoded].map((c) => parseInt(c, 16))]);
    }

    const encodedPath = params.get("path");

    if (encodedPath !== null) {
        const match = encodedPath.match(/^(\d+)\.(\d+)(?:-(.*))?$/);

        if (match) {
            const [, startNgon, startNth, turns = ""] = match;

            uiState.value.path = {
                start_ngon: Number(startNgon),
                start_nth: Number(startNth),
                turns: [...turns].map((c) => parseInt(c, 16)),
            };
        }
    }

    if (entries.length > 0 && entries !== uiState.value.variant_map) {
        uiState.value.variant_map = entries;
    }

    if (
        polyhedron &&
        polyhedra.value.includes(polyhedron) &&
        uiState.value.polyhedron != polyhedron
    ) {
        console.log(
            "setting polyhedron to ",
            polyhedron,
            " because ",
            uiState.value.polyhedron,
            " is different ",
        );
        uiState.value.polyhedron = polyhedron;
    } else {
        console.log("could not find ", polyhedron);
        // do nothing
    }
}

onMounted(async () => {
    const wasm = await import("./pkg/poly_pcb.js");

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
    uiState,
    async (state) => {
        if (iface) {
            console.log(
                "setting poly ",
                state.polyhedron,
                "with variant map",
                state.variant_map,
            );
            const [missing_variants, corrected_design] = iface.set_polyhedron({
                SinglePoly: uiState.value,
            });
            if (corrected_design !== null) {
                state = corrected_design.SinglePoly;
            }
            console.log("missing variants", missing_variants);
            pcbLoader.value!.requestMany(missing_variants);
        }
        update_url();
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

    let entry = uiState.value.variant_map.find(([n]) => n === n_gon);

    if (!entry) {
        entry = [n_gon, []];
        uiState.value.variant_map.push(entry);
    }

    const variants = entry[1];

    while (variants.length <= nth_ngon) {
        variants.push(0);
    }

    variants[nth_ngon] = variant;
}
</script>

<template>
    <div class="canvas-container">
        <header>
            <button :disabled="mode === steps[0]" @click="mode--">&lt;</button>

            <template v-for="(step, i) in steps" :key="step">
                <div>
                    <button
                        :disabled="step < mode"
                        :style="{
                            fontWeight: step === mode ? 'bold' : 'normal',
                        }"
                        @click="mode = step"
                    >
                        {{ i + 1 }}. {{ CurrentStep[step] }}
                    </button>

                    <select
                        v-if="step === CurrentStep.SelectPoly && mode === step"
                        v-model="uiState.polyhedron"
                    >
                        <option v-for="name in polyhedra" :key="name">
                            {{ name }}
                        </option>
                    </select>
                </div>

                <span v-if="i < steps.length - 1">────</span>
            </template>

            <button
                :disabled="mode === steps[steps.length - 1]"
                @click="mode++"
            >
                &gt;
            </button>
        </header>
        <canvas
            ref="canvas"
            tabindex="0"
            @keydown="iface.on_key"
            @next_polyhedron="uiState.polyhedron = $event.detail"
            @request_pcb="
                (e: CustomEventInit<VarId>) => {
                    on_request_pcb(e.detail!);
                }
            "
            @design_changed="
                (e: CustomEventInit<PcbDesign>) => (uiState = e.detail!)
            "
            @pointerdown="iface?.on_pointer_down"
            @pointermove="iface?.on_pointer_move"
            @pointerup="iface?.on_pointer_up"
            @wheel.prevent="iface?.on_wheel"
            @click="iface?.on_click"
            @dblclick="iface?.next_polyhedron"
        ></canvas>
    </div>
</template>

<style>
.canvas-container {
    height: 100%;
    width: 100%;
    position: relative;
    z-index: 0;
}

.canvas-container canvas {
    position: absolute;
    top: 0;
    width: 100%;
    height: 100%;
    z-index: 0;
    display: block;
}

button,
select {
    padding: 0.5rem 1rem;
    background: #2ec27e;
    border-radius: 1rem;
    border: 1px solid #26a269;
}

select option {
    background: #2ec27e;
    color: #fff;
}

header {
    position: absolute;
    inset: 0 0 auto 0;
    z-index: 100;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 1rem;
    padding: 1rem;

    backdrop-filter: blur(8px);
    pointer-events: all;
}
</style>
