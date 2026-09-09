<script setup lang="ts">
import { ref, onMounted, watch, type Ref, computed, onUnmounted } from "vue";
import {
    type CurrentStep,
    Interface,
    PcbId,
    VarId,
    type PcBorsign,
    type Steps,
    type VarFlags,
    type MissingVariants,
    type PcbPath,
    type PcbPaths,
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
const design = ref<PcBorsign>({
    polyhedra: [],
    variant_map: [],
    path: []
});
let iface: Interface;
const allSteps: Ref<CurrentStep[]> = ref([]);
const allVariants: Ref<VarFlags[]> = ref([]);
const currentStep = computed<CurrentStep | undefined>({
  get() {
    if (mode.value === 1) {
      return { AssignVariants: currentVar.value };
    } else {
      return allSteps.value[mode.value];
    }
  },
    set(step) {
        mode.value = allSteps.value.findIndex(s => s === step || (typeof step === 'object' && 'AssignVariants' in step && typeof s === 'object' && 'AssignVariants' in s));

    }
});
const currentVariant: Ref<number[]> = ref([]);

const currentVar = computed<number>({
    get() {
        return currentVariant.value.reduce(
            (mask, variant) => mask | (1 << variant),
            0,
        );
    },
    set(mask) {
        currentVariant.value = allVariants.value
            .map((_, i) => i)
            .filter(i => mask & (1 << i));
    },
});

window.addEventListener("hashchange", () => {
    apply_url();
});

/// update url to match current design
function update_url() {
    const names = design.value.polyhedra;
    const map = design.value.variant_map;
    const paths = design.value.path;
    let hash = `#/${names.map(name=>name.replace(/ /g, "-")).join("|")}`;

    const params = new URLSearchParams();

    for (const [nGon, variants] of map) {
        params.set(
            nGon.toString(),
            variants.map((v) => v.toString(16)).join(""),
        );
    }
  let path = paths.map(path => `${path.start_ngon}.${path.start_nth}-${path.turns.map((t) => t.toString(36)).join("")}`).join("_");
  if (path) {
    params.set("path", path);
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
    const polyhedra = polyUrl?.toLowerCase().replace(/[-_ ]+/g, " ").split("|");
    const entries: [number, number[]][] = [];
    const params = new URLSearchParams(query);

    for (const [key, encoded] of params) {
        const nGon = Number(key);
        if (!Number.isInteger(nGon) || nGon < 3 || nGon > 10) continue;
        entries.push([Number(nGon), [...encoded].map((c) => parseInt(c, 16))]);
    }


    if (entries.length > 0 && entries !== design.value.variant_map) {
        design.value.variant_map = entries;
    }

    const encodedPath = params.get("path");

    if (encodedPath !== null) {
        design.value.path = encodedPath.split("_").map(encoded => {
            const [startNgon, startNthAndTurns] = encoded.split(".");
            const [startNth, turns = ""] = startNthAndTurns!.split("-");

            return {
                start_ngon: Number(startNgon),
                start_nth: Number(startNth),
                turns: [...turns].map(c => parseInt(c, 36)),
            };
        });
    }


    if (
        polyhedra
    ) {
        console.log(
            "setting polyhedron to ",
            polyhedra,
            " because ",
            design.value.polyhedra,
            " is different ",
        );
      design.value.polyhedra = polyhedra;
      set_design(design.value);
    } else {
        console.log("could not find ", polyhedra);
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
    });
    ro.observe(canvas.value);
    apply_url();
    window.addEventListener("keydown", event => iface?.on_key(event));
});

onUnmounted(async () => {
      window.removeEventListener("keydown", event => iface?.on_key(event));
})

// this is sad because it's now bidirectional:
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

function set_polyhedron(polyhedron: string, index: number) {
  if (!iface) {
    return;
  }
  design.value.polyhedra[index] = polyhedron;
  pcbLoader.value!.requestMany(iface.set_polyhedron(polyhedron, index)![1]);
}


function push_polyhedron(polyhedron: string) {
  if (!iface) {
    return;
  }
  design.value.polyhedra.push(polyhedron);
  pcbLoader.value!.requestMany(iface.push_polyhedron(polyhedron)![1]);
}

function do_pop_polyhedron() {
  if (!iface) {
    return;
  }
  pop_polyhedron();
  iface.pop_polyhedron();
}

function pop_polyhedron() {
  console.log("popping poly");
  design.value.polyhedra.pop();
}

function on_update_polyhedron(missing_variants: MissingVariants, index: number) {
  design.value.polyhedra[index] = missing_variants[0];
  pcbLoader.value!.requestMany(missing_variants![1]);
}

function set_design(new_design: PcBorsign) {
  if (!iface) {
    console.warn("setting design without initialized iface")
    return;
  }
  const [missing_variants, corrected_design] = iface.apply_design(new_design);
  if (corrected_design !== null) {
      design.value = corrected_design;
  } else {
    design.value = new_design;
  }
  console.log("missing variants", missing_variants);
  pcbLoader.value!.requestMany(missing_variants);
}

function on_update_variant(var_id: VarId) {
    console.log("request pcb", var_id);

    // check if there is actually an stl for the requested variant???? otherwise cycle to 0
    const { nth_ngon, pcb_id } = var_id;
    let { n_gon, variant } = pcb_id;

    console.log("requested pcb for ", n_gon, variant);
    if (!pcbLoader.value?.pcb_exists(n_gon, variant)) {
        console.warn(`pcb ${n_gon} version ${variant} does not exist`);
      var_id.pcb_id.variant = 0;
      iface.update_variant(var_id);
    } else {
      pcbLoader.value?.loadOne(n_gon, variant)
    }

    let entry = design.value.variant_map.find(([n]) => n === n_gon);
    if (!entry) {
        console.warn(`entry for ${n_gon} does not exist yet, creating`)
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

function on_update_path(path: PcbPaths | undefined) {
  if (path === undefined) {
    design.value.path = []
  } else {
    design.value.path = path
  }

}

function animate(timestamp: number) {
  if (iface?.animate(timestamp)) {
    requestAnimationFrame(animate)
  }
}

function start_animation() {
  requestAnimationFrame(animate)
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

                    <div v-if="step === 'SelectPoly' && mode === i" class="polyhedron-selects">
                        <button v-if="design.polyhedra.length > 0" @click="do_pop_polyhedron()">-</button>
                        <select
                            v-for="(name, j) in design.polyhedra"
                            :key="j"
                            :value="name"
                            @change="set_polyhedron(
                                ($event.target as HTMLSelectElement).value,
                                j
                            )"
                        >
                            <option
                                v-for="name in polyhedra"
                                :key="name"
                                :value="name"
                            >
                                {{ name }}
                            </option>
                        </select>

                        <select
                            value="+"
                            @change="push_polyhedron(($event.target as HTMLSelectElement).value);
                                         ($event.target as HTMLSelectElement).value = '+'"
                        >
                            <option value="+" disabled>+</option>
                            <option
                                v-for="name in polyhedra"
                                :key="name"
                                :value="name"
                            >
                                {{ name }}
                            </option>
                        </select>
                    </div>
                    <div
                    class="variant-menu"
                        v-else-if="typeof step === 'object' && 'AssignVariants' in step && mode === i"
                    >
                        <label v-for="(variant,i) in allVariants">
                            <input type="checkbox" :value="i" v-model="currentVariant"> {{ variant }} </input>
                        </label>
                    </div>
                    <div
                    class="variant-menu"
                        v-else-if="step === 'MakePath' && mode === i"
                    >
                        <label v-for="(variant,i) in design.polyhedra">
                            <input type="checkbox" :value="i" v-model="showPolys"> {{ variant }} </input>
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
            @pointerdown="iface?.on_pointer_down"
            @pointermove="iface?.on_pointer_move"
            @pointerup="iface?.on_pointer_up"
            @wheel.prevent="iface?.on_wheel"
            @click="iface?.on_click"

            @design_changed="
                (e: CustomEventInit<PcBorsign>) => (design = e.detail!)
            "
            @next_polyhedron="(e: CustomEventInit<MissingVariants>) => {on_update_polyhedron(e.detail!, 0)}"
            @pop_polyhedron="pop_polyhedron()"
            @update_step="(e: CustomEventInit<CurrentStep>) => currentStep = e.detail!"

            @update_current_var="(e: CustomEventInit<number>) => { currentVar = e.detail! }"
            @update_variant="
                (e: CustomEventInit<VarId>) => {
                    on_update_variant(e.detail!);
                }
            "

            @update_path="(e: CustomEventInit<PcbPaths>) => {
              on_update_path(e.detail);
            } "

            @start_animation="(e:CustomEventInit<number>) => (start_animation())"

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

.polyhedron-selects {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    width: 100%;
}

header {
    position: absolute;
    inset: 0 0 auto 0;
    z-index: 100;
    display: flex;
    /*align-items: center;*/
    /*justify-content: center;*/
    justify-content: space-between;
    gap: 1rem;
    padding: 1rem;

    backdrop-filter: blur(2px);
    /*pointer-events: all;*/
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
