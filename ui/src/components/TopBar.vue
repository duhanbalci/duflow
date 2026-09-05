<script setup lang="ts">
import { computed, ref } from 'vue'
import { search, type Hit } from '../graph'
import { graph, state, jumpTo, startReplay, stopReplay, back, toggleLayer, startAt, current } from '../walk'

const q = ref('')
const open = ref(false)
const sel = ref(0)
const input = ref<HTMLInputElement>()
const hits = computed<Hit[]>(() => (graph.value && open.value ? search(graph.value, q.value) : []))

function hl(s: string, pos: number[]) {
  const set = new Set(pos)
  return [...s].map((c, i) => (set.has(i) ? `<b>${esc(c)}</b>` : esc(c))).join('')
}
function esc(s: string) { return s.replace(/&/g, '&amp;').replace(/</g, '&lt;') }

function closeLater() { window.setTimeout(() => (open.value = false), 120) }
function pick(h: Hit | undefined) {
  if (!h?.go) return
  jumpTo(h.go)
  q.value = ''; open.value = false; input.value?.blur()
}
function onKey(e: KeyboardEvent) {
  if (e.key === 'ArrowDown') { e.preventDefault(); sel.value = Math.min(sel.value + 1, hits.value.length - 1) }
  else if (e.key === 'ArrowUp') { e.preventDefault(); sel.value = Math.max(sel.value - 1, 0) }
  else if (e.key === 'Enter') pick(hits.value[sel.value])
  else if (e.key === 'Escape') { q.value = ''; open.value = false; input.value?.blur() }
}
window.addEventListener('keydown', (e) => {
  if ((e.metaKey || e.ctrlKey) && e.key === 'k') { e.preventDefault(); input.value?.focus(); open.value = true }
})

const roles = computed(() => graph.value?.vars.get('role')?.values ?? [])
const rootOptions = computed(() => graph.value?.roots ?? [])

/** Replay hedefi: şimdiki node'a root'tan yol */
function replayToCurrent() {
  const g = graph.value; if (!g) return
  if (state.replay) { stopReplay(); return }
  const p = g.pathFromRoots(current.value)
  if (p && p.length > 1) startReplay(p)
}
const views = computed(() => graph.value?.data.views ?? [])
function playView(id: string) {
  const g = graph.value; const v = views.value.find((x) => x.id === id); if (!g || !v) return
  const p = g.shortestPath(v.from, v.to); if (p) startReplay(p)
}
</script>

<template>
  <header class="bar">
    <div class="brand">duflow <small>· {{ graph?.data.project.name }}</small></div>

    <div class="search" @focusout="closeLater">
      <span class="ico">⌕</span>
      <input ref="input" v-model="q" placeholder="node, check, var ara…" autocomplete="off"
        @focus="open = true" @input="sel = 0; open = true" @keydown="onKey" />
      <kbd>⌘K</kbd>
      <div v-if="open && q.trim()" class="pal">
        <div v-if="!hits.length" class="none">eşleşme yok</div>
        <template v-for="(h, i) in hits" :key="h.kind + h.id">
          <div v-if="i === 0 || hits[i - 1].kind !== h.kind" class="sec">{{ h.kind }}</div>
          <div class="it" :class="{ sel: i === sel }" :data-layer="h.layer" @mousedown.prevent="pick(h)" @mousemove="sel = i">
            <span class="dot"></span>
            <span class="id mono" v-html="hl(h.id, h.pos)"></span>
            <span class="d">{{ h.desc }}</span>
          </div>
        </template>
      </div>
    </div>

    <div class="layers">
      <button v-for="l in graph?.layers" :key="l" class="lay" :data-layer="l" :class="{ off: state.hiddenLayers.has(l) }"
        :title="`${l} katmanını göster/gizle`" @click="toggleLayer(l)">
        <span class="dot"></span>{{ l }}
      </button>
    </div>

    <label class="role" v-if="roles.length">
      <span>rol</span>
      <select v-model="state.role" @change="state.vars['role'] = state.role || '—'">
        <option value="">hepsi</option>
        <option v-for="r in roles" :key="r" :value="r">{{ r }}</option>
      </select>
    </label>

    <div class="views">
      <button :class="{ on: state.view === 'walk' }" @click="state.view = 'walk'">Walk</button>
      <button :class="{ on: state.view === 'map' }" @click="state.view = 'map'" title="m">Map</button>
    </div>

    <div class="replay">
      <select class="root" :value="''" @change="startAt(($event.target as HTMLSelectElement).value); ($event.target as HTMLSelectElement).value = ''">
        <option value="" disabled>root'tan başla…</option>
        <option v-for="r in rootOptions" :key="r" :value="r">{{ r }}</option>
      </select>
      <select v-if="views.length" class="root" :value="''" @change="playView(($event.target as HTMLSelectElement).value); ($event.target as HTMLSelectElement).value = ''">
        <option value="" disabled>view oynat…</option>
        <option v-for="v in views" :key="v.id" :value="v.id">{{ v.id }}</option>
      </select>
      <span v-if="state.replay" class="step mono">{{ state.replay.idx }}/{{ state.replay.path.length - 1 }}</span>
      <button @click="stopReplay(); back()" title="←">← geri</button>
      <button class="primary" @click="replayToCurrent">{{ state.replay ? '⏸ durdur' : '▶ buraya nasıl gelinir' }}</button>
    </div>
  </header>
</template>

<style scoped>
.bar { display: flex; align-items: center; gap: 14px; padding: 8px 16px; border-bottom: 1px solid var(--line); background: var(--surface); flex: 0 0 auto; }
.brand { font-weight: 600; letter-spacing: .02em; white-space: nowrap; }
.brand small { font-weight: 400; color: var(--muted); }
.search { position: relative; flex: 1; max-width: 420px; display: flex; align-items: center; gap: 8px; background: var(--surface2); border: 1px solid var(--line); border-radius: 8px; padding: 5px 10px; color: var(--muted); }
.search input { flex: 1; background: none; border: 0; outline: 0; min-width: 0; }
.search kbd { font-size: 11px; border: 1px solid var(--line); border-radius: 4px; padding: 0 5px; color: var(--faint); }
.pal { position: absolute; top: calc(100% + 6px); left: 0; right: 0; background: var(--surface); border: 1px solid var(--line); border-radius: 10px; box-shadow: var(--shadow); z-index: 20; max-height: 380px; overflow: auto; padding: 6px; }
.pal .sec { font-size: 10px; letter-spacing: .08em; text-transform: uppercase; color: var(--faint); padding: 6px 8px 2px; }
.pal .it { display: flex; align-items: center; gap: 8px; padding: 6px 8px; border-radius: 6px; cursor: pointer; }
.pal .it.sel { background: var(--surface2); }
.pal .it .id { font-size: 12px; color: var(--text); }
.pal .it .id :deep(b) { font-weight: 600; color: var(--ui); }
.pal .it .d { color: var(--muted); font-size: 12px; margin-left: auto; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 45%; }
.pal .none { padding: 10px; color: var(--faint); font-size: 12px; }
.layers { display: flex; gap: 4px; }
.lay { display: flex; align-items: center; gap: 6px; font-size: 12px; padding: 4px 9px; border-radius: 6px; border: 1px solid var(--line); color: var(--text); }
.lay.off { color: var(--faint); border-style: dashed; }
.lay.off .dot { opacity: .35; }
.role { display: flex; align-items: center; gap: 6px; font-size: 12px; color: var(--muted); }
select { font: inherit; font-size: 12px; color: var(--text); background: var(--surface2); border: 1px solid var(--line); border-radius: 6px; padding: 4px 6px; }
.views { display: flex; border: 1px solid var(--line); border-radius: 6px; overflow: hidden; }
.views button { font-size: 12px; padding: 4px 10px; color: var(--muted); }
.views button.on { background: var(--now); color: var(--bg); }
.replay { display: flex; align-items: center; gap: 6px; margin-left: auto; }
.replay button { padding: 5px 10px; border-radius: 6px; border: 1px solid var(--line); color: var(--muted); font-size: 13px; white-space: nowrap; }
.replay button.primary { background: var(--now); color: var(--bg); border-color: var(--now); }
.replay .step { color: var(--faint); font-size: 12px; }
</style>
