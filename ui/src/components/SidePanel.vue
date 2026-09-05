<script setup lang="ts">
import { computed } from 'vue'
import { graph, state, rewindTo, stopReplay, setVar } from '../walk'
import { ref } from 'vue'

const editing = ref('')
function commit(id: string, e: Event) { setVar(id, (e.target as HTMLInputElement).value.trim()); editing.value = '' }

const vars = computed(() => [...(graph.value?.vars.values() ?? [])])
const trail = computed(() => state.hist.slice(-8))
const diff = computed(() => graph.value?.diff)
</script>

<template>
  <aside class="side">
    <section>
      <h3>Değişkenler</h3>
      <div class="vars">
        <div v-for="v in vars" :key="v.id" class="var" :class="{ flash: state.flash === v.id }" :title="v.desc">
          <span><span class="k mono">{{ v.id }}</span> <span class="src">· {{ v.source ?? 'sets' }}</span></span>
          <input v-if="editing === v.id" class="v mono edit" :value="state.vars[v.id]" :list="'dl-' + v.id" autofocus
            @keydown.enter="commit(v.id, $event)" @keydown.esc="editing = ''" @blur="commit(v.id, $event)" />
          <button v-else class="v mono" title="değeri değiştir (ya şöyle olsaydı)" @click="editing = v.id">{{ state.vars[v.id] ?? '—' }}</button>
          <datalist v-if="v.values?.length" :id="'dl-' + v.id"><option v-for="o in v.values" :key="o" :value="o" /></datalist>
        </div>
      </div>
    </section>
    <section>
      <h3>Yürüyüş</h3>
      <div class="trail">
        <button v-for="(id, i) in trail" :key="i + id" class="mono" :class="{ last: i === trail.length - 1 }" @click="stopReplay(); rewindTo(id)">{{ id }}</button>
      </div>
    </section>
    <section v-if="diff">
      <h3>Diff</h3>
      <div class="diff">
        <span class="chip added">+{{ diff.added.length }}</span>
        <span class="chip removed">−{{ diff.removed.length }}</span>
        <span class="chip changed">~{{ diff.changed.length }}</span>
      </div>
    </section>
    <div class="hint">Sağdaki adaya gel, devamı ufukta belirir. Tıkla, akış oraya kayar. Geçmiş karta tıkla: geri sar. <kbd>←</kbd> geri, <kbd>⌘K</kbd> ara, <kbd>m</kbd> harita.</div>
  </aside>
</template>

<style scoped>
.side { width: 280px; border-left: 1px solid var(--line); background: var(--surface); padding: 16px; display: flex; flex-direction: column; gap: 18px; overflow: auto; flex: 0 0 auto; }
h3 { margin: 0 0 8px; font-size: 11px; letter-spacing: .08em; text-transform: uppercase; color: var(--muted); font-weight: 500; }
.var { display: flex; justify-content: space-between; gap: 8px; padding: 6px; border-radius: 6px; font-size: 12px; border-bottom: 1px solid var(--line); }
.var .k { color: var(--muted); } .var .v { color: var(--text); padding: 0 4px; border-radius: 4px; }
.var button.v:hover { background: var(--surface2); }
.var .edit { width: 90px; background: var(--surface2); border: 1px solid var(--line); font-size: 12px; outline: 0; } .var .src { color: var(--faint); font-size: 11px; }
.var.flash { animation: flash 1.1s ease-out; }
@keyframes flash { 0% { background: var(--domain); color: var(--bg); } 100% { background: transparent; } }
.trail { display: flex; flex-direction: column; }
.trail button { text-align: left; font-size: 12px; padding: 3px 0 3px 12px; border-left: 2px solid var(--line); color: var(--muted); }
.trail button.last { border-left-color: var(--now); color: var(--text); }
.trail button:hover { color: var(--text); }
.diff { display: flex; gap: 6px; }
.hint { font-size: 12px; color: var(--faint); line-height: 1.5; margin-top: auto; }
kbd { font-size: 11px; border: 1px solid var(--line); border-radius: 4px; padding: 0 4px; }
</style>
