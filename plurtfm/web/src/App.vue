<script setup lang="ts">
import { ref, onMounted, watch, type Ref, computed } from "vue";
import {
    type CurrentStep,
    Interface,
    PcbId,
    VarId,
    type PcbDesign,
    type Steps,
    type VarFlags,
    type LampDesign,
    type MissingVariants,
    type PcbPath,
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
const mode = ref<number>(0);
const design = ref<PcbDesign>({
    polyhedron: "tetrahedron",
    variant_map: [[3, [4, 0, 1, 2]]],
    path: { start_ngon: 3, start_nth: 0, turns: [] },
});
let iface: Interface;
const allSteps: Ref<CurrentStep[]> = ref([]);
const allVariants: Ref<VarFlags[]> = ref([]);
const currentStep = computed<CurrentStep | undefined>(() => {
  if (mode.value === 1) {
    return { AssignVariants: currentVar.value };
  } else {
    return allSteps.value[mode.value];
}
});
const currentVariant: Ref<number[]> = ref([]);
const currentVar = computed<number>(() => {
  return currentVariant.value.reduce((mask, variant) => mask | (1 << variant), 0);
});

window.addEventListener("hashchange", () => {
    apply_url();
});

/// update url to match current design
function update_url() {
    const name = design.value.polyhedron;
    const map = design.value.variant_map;
    const path = design.value.path;
    let hash = `#/${name.replace(/ /g, "-")}`;

    const params = new URLSearchParams();

    for (const [nGon, variants] of map) {
        params.set(
            nGon.toString(),
            variants.map((v) => v.toString(16)).join(""),
        );
    }
    if (path?.turns) {
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

/// Apply the url, that is update the design based on the url
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

            design.value.path = {
                start_ngon: Number(startNgon),
                start_nth: Number(startNth),
                turns: [...turns].map((c) => parseInt(c, 16)),
            };
        }
    }

    if (entries.length > 0 && entries !== design.value.variant_map) {
        design.value.variant_map = entries;
    }

    if (
        polyhedron &&
        polyhedra.value.includes(polyhedron) &&
        design.value.polyhedron != polyhedron
    ) {
        console.log(
            "setting polyhedron to ",
            polyhedron,
            " because ",
            design.value.polyhedron,
            " is different ",
        );
      set_design({SinglePoly: design.value });
    } else {
        console.log("could not find ", polyhedron);
        // do nothing
    }
}

onMounted(async () => {
    const wasm = await import("./pkg/poly_pcb.js");

    await wasm.default();
    let db = await loadAsset("polydb.sqlite3");
    allSteps.value = wasm.steps();
    allVariants.value = wasm.var_flags();
    console.log(allSteps);
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

/// this is sad because it's now bidirectional:
// 1. update url based on design
// 2. update iface based on url
// 3. update url and iface based on button inputs
// and as a result, iface will also be updated if the change comes from there
// The better would be if there's only two, so the kinda logical thing to do is put it in iface?
//
watch(
    design,
    async (state) => {
        update_url();
    },
    { deep: true },
);

watch(
  currentStep,
  (step) => {
    if (iface && step) {
      iface.set_step(step);
    }
  }
);


watch(
  currentVar,
  (v) => {
    // set the mode to assignvars
    mode.value = 1;
  }
);

function set_polyhedron(polyhedron: string) {
  if (!iface) {
    return;
  }
  design.value.polyhedron = polyhedron;
  pcbLoader.value!.requestMany(iface.set_polyhedron(polyhedron)![1]);
}

function on_update_polyhedron(missing_variants: MissingVariants) {
  design.value.polyhedron = missing_variants[0];
  pcbLoader.value!.requestMany(missing_variants![1]);
}

function set_design(new_design: LampDesign) {
  if (!iface) {
    return;
  }
  const [missing_variants, corrected_design] = iface.apply_design(new_design);
  if (corrected_design !== null) {
      design.value = corrected_design.SinglePoly;
  } else {
    design.value = new_design.SinglePoly;
  }
  console.log("missing variants", missing_variants);
  pcbLoader.value!.requestMany(missing_variants);
}

function on_update_variant(var_id: VarId) {
    console.log("request pcb", var_id);

    // check if there is actually an stl for the requested variant???? otherwise cycle to 0
    const { nth_ngon, pcb_id, need_fetch } = var_id;
    let { n_gon, variant } = pcb_id;

    console.log("requested pcb for ", n_gon, variant);
    if (!pcbLoader.value?.pcb_exists(n_gon, variant) && need_fetch) {
        console.warn(`pcb ${n_gon} version ${variant} does not exist`);
      variant = 0;
      iface.update_variant(n_gon, nth_ngon, variant);
    } else if (need_fetch) {
      pcbLoader.value?.loadOne(n_gon, variant)
    }

    let entry = design.value.variant_map.find(([n]) => n === n_gon);
    if (!entry) {
        entry = [n_gon, []];
        design.value.variant_map.push(entry);
    }
    const variants = entry[1];
    while (variants.length <= nth_ngon) {
        variants.push(0);
    }
    variants[nth_ngon] = variant;
    // so I'm not sure if we need to update the url now, or we're just happy
}

function on_update_path(path: PcbPath | undefined) {
  if (path === undefined) {
    design.value.path = null
  } else {
    design.value.path = path
  }

}

</script>

<template>
    <div class="canvas-container">
        <header>
            <button :disabled="mode === 0" @click="mode--">&lt;</button>

            <template v-for="(step, i) in allSteps">
                <div>
                    <button
                        :class="{
                            current: i === mode,
                            previous: i < mode,
                            next: i > mode,
                        }"
                        :disabled="i < mode"
                        :style="{
                            fontWeight: i === mode ? 'bold' : 'normal',
                        }"
                        @click="mode = i"
                    >
                        {{ i + 1 }}. {{ allSteps[i] }}
                    </button>

                    <select
                        v-if="step === 'SelectPoly' && mode === i"
                            :value="design.polyhedron"
                        @change="set_polyhedron(($event.target as HTMLSelectElement).value)"
                    >
                        <option v-for="name in polyhedra" :key="name">
                            {{ name }}
                        </option>
                    </select>
                    <div
                    class="variant-menu"
                        v-else-if="typeof step === 'object' && 'AssignVariants' in step && mode === i"
                    >
                        <label v-for="(variant,i) in allVariants">
                            <input type="checkbox" :value="i" v-model="currentVariant"> {{ variant }} </input>
                        </label>
                    </div>
                    <div class="path-menu" v-else-if="step === 'MakePath' && mode === i">
                        <button  @click="iface.complete_path()" >Find path</button>
                        <button @click="iface.pop_path()">Back</button>
                    </div>
                </div>
            </template>

            <button :disabled="mode === allSteps?.length - 1" @click="mode++">
                &gt;
            </button>
        </header>
        <canvas
            ref="canvas"
            tabindex="0"
            @keydown="iface.on_key"
            @next_polyhedron="(e: CustomEventInit<MissingVariants>) => {on_update_polyhedron(e.detail!)}"
            @update_variant="
                (e: CustomEventInit<VarId>) => {
                    on_update_variant(e.detail!);
                }
            "
            @update_path="(e: CustomEventInit<PcbPath>) => {
              on_update_path(e.detail);
            } "
            @design_changed="
                (e: CustomEventInit<PcbDesign>) => (design = e.detail!)
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
    touch-action: pinch-zoom;
}

button,
select {
    padding: 0.5rem 1rem;
    background: #2ec27e;
    border-radius: 1rem;
    border: 2px solid #26a269;
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
    /*justify-content: center;*/
    justify-content: space-between;
    gap: 1rem;
    padding: 1rem;

    backdrop-filter: blur(8px);
    pointer-events: all;
}

header select {
    width: 100%;
    min-width: 0;
}


.variant-menu {
    position: absolute;
    top: 100%;
    left: 0;
    display: flex;
    flex-direction: column;
    padding: 0.5rem;
    background: white;
    border: 1px solid #ccc;
    border-radius: 0.5rem;
}

.variant-menu label {
    padding: 0.25rem 0.5rem;
    white-space: nowrap;
}

@media (max-width: 600px) {
    .step:not(.current) {
        display: none;
    }

    header .previous,
    header .next {
        display: none;
    }
}
</style>
