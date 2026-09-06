<script setup lang="ts">
import { computed, onBeforeUpdate, onMounted, onUpdated, ref } from 'vue'
import type { Candidate } from '../graph'
import { graph } from '../walk'

const props = defineProps<{
  id: string
  col: 'past' | 'now' | 'next' | 'horizon'
  cand?: Candidate
  hot?: boolean
  /** kapalı: yükseklik 0 + saydam (seçilmeyen kardeşler) */
  collapsed?: boolean
  /** mount'ta bu yükseklikten gerçek yüksekliğe animasyon (slot değişiminde süreklilik) */
  fromHeight?: number
}>()

// ---- yükseklik animasyonu: içerik/stil değişince (col, cand, collapsed) eski → yeni ----
const el = ref<HTMLElement>()
let h0 = 0
// onBeforeUpdate anında prop'lar zaten yeni; önceki şekli kendimiz saklarız
const shapeKey = () => `${props.col}|${props.collapsed ? 1 : 0}|${props.cand ? 1 : 0}`
let lastId = props.id
let lastShape = shapeKey()
onBeforeUpdate(() => { h0 = el.value?.offsetHeight ?? 0 })
// yalnız aynı node'un stili/kapalılığı değişince animasyon; id değişimi (bileşen yeniden kullanımı) asla
onUpdated(() => {
  const shape = shapeKey()
  if (props.id === lastId && shape !== lastShape) animateHeight(h0)
  lastId = props.id; lastShape = shape
})
onMounted(() => { if (props.fromHeight) animateHeight(props.fromHeight); else if (props.collapsed && el.value) { el.value.style.height = '0px'; el.value.style.overflow = 'hidden' } })
// Stil değişimleri (padding, font, kenarlık) anında uygulanır; yalnız yükseklik/opaklık/margin animasyonlu.
// Hedef yükseklik, transition'lar kapalıyken ölçülür → ara değer ölçme hatası yok.
let cleanup: (() => void) | null = null
function animateHeight(from: number) {
  const e = el.value; if (!e) return
  cleanup?.()
  const op0 = getComputedStyle(e).opacity
  e.style.transition = 'none'; e.style.height = ''; e.style.opacity = ''
  void e.offsetHeight
  const to = props.collapsed ? 0 : e.offsetHeight
  if (Math.abs(to - from) < 1) { e.style.transition = ''; if (props.collapsed) { e.style.height = '0px'; e.style.overflow = 'hidden' } return }
  e.style.overflow = 'hidden'; e.style.height = `${from}px`; e.style.opacity = op0
  void e.offsetHeight
  e.style.transition = 'height .42s cubic-bezier(.2,.8,.2,1), opacity .3s, margin .42s'
  e.style.height = `${to}px`; e.style.opacity = ''
  const done = (ev: TransitionEvent) => { if (ev.propertyName === 'height') finish() }
  const finish = () => { e.removeEventListener('transitionend', done); cleanup = null; if (!props.collapsed) { e.style.height = ''; e.style.overflow = '' } e.style.transition = '' }
  cleanup = finish
  e.addEventListener('transitionend', done)
}

const g = graph
const node = computed(() => g.value?.card(props.id))
const full = computed(() => g.value?.get(props.id))
const diffTag = computed(() => {
  if (props.cand?.diff === 'removed' || node.value?.removed) return 'removed'
  if (props.cand?.diff === 'added' || g.value?.added.has(props.id)) return 'added'
  if (g.value?.changed.has(props.id)) return 'changed'
  return ''
})
const listens = computed(() => (full.value?.out ?? []).filter((e) => e.label.startsWith('on ')).map((e) => e.label.slice(3)))
const isRoot = computed(() => g.value?.roots.includes(props.id))
const every = computed(() => g.value?.every[props.id])
/** `requires x` → perm:x check'i; chip'te "requires x" yazar */
const checkLabel = (name: string) => name.startsWith('perm:') ? 'requires ' + name.slice(5) : 'check ' + name
</script>

<template>
  <div ref="el" class="card" :role="col === 'now' ? undefined : 'button'" :class="[col, { hot, collapsed, removed: diffTag === 'removed', unmet: cand?.guard === 'false', met: cand?.guard === 'true' }]"
    :data-id="id" :data-layer="node?.layer" :data-cls="cand?.cls ?? ''" :data-guard="cand?.guard ?? ''" :data-labels="JSON.stringify(cand?.labels.map(l => ({ label: l.label, cls: l.cls })) ?? [])"
    :tabindex="col === 'now' || col === 'horizon' ? -1 : 0">
    <div class="kind">
      <span class="dot"></span>{{ node?.kind }}
      <span v-if="isRoot" class="root">root<template v-if="every"> · every {{ every }}</template></span>
      <span v-if="diffTag" class="chip" :class="diffTag">{{ diffTag === 'added' ? 'new' : diffTag === 'removed' ? 'removed' : 'changed' }}</span>
    </div>
    <div class="id mono">{{ id }}</div>
    <div class="desc">{{ node?.desc }}</div>
    <div v-if="cand?.labels.length && col !== 'horizon'" class="lbls">
      <span v-for="l in cand.labels" :key="l.label" class="lbl" :class="l.cls">{{ l.label }}</span>
      <span v-if="cand.guard === 'true'" class="g met">✓ currently met</span>
      <span v-else-if="cand.guard === 'false'" class="g unmet">✗ currently unmet</span>
    </div>

    <template v-if="col === 'now' && full">
      <div class="meta" v-if="Object.keys(full.attrs).length || full.checks.length || full.sets.length || listens.length">
        <span v-for="(v, k) in full.attrs" :key="k" class="chip attr mono">{{ k }} {{ v }}</span>
        <span v-for="c in full.checks" :key="c.name" class="chip check" :title="g?.checks.get(c.name)?.desc">
          {{ checkLabel(c.name) }}<template v-if="c.fail"> ✗ {{ c.fail }}</template>
        </span>
        <span v-for="s in full.sets" :key="s.var" class="chip set">sets {{ s.var }} {{ s.value }}</span>
        <span v-for="ev in listens" :key="ev" class="chip attr">listens {{ ev }}</span>
      </div>
      <div v-if="full.outcomes?.length" class="outcomes">
        <span v-for="o in full.outcomes" :key="o.label + o.text" class="outcome" :class="o.class">
          <span class="mono">{{ o.label }}</span> ⇥ {{ o.text }}
        </span>
      </div>
      <div class="file mono">{{ full.file }}</div>
      <div v-if="full.doc" class="doc">{{ full.doc }}</div>
    </template>

    <div v-if="cand?.via" class="via mono">
      via {{ cand.via.kind === 'call' ? (cand.via.attrs.method ?? '') + ' ' + (cand.via.attrs.path ?? cand.via.id) : cand.via.id }}
      <template v-if="cand.via.checks.length"> · {{ cand.via.checks.length }} check</template>
    </div>
  </div>
</template>

<style scoped>
.card { position: relative; box-sizing: border-box; background: var(--surface); border: 1px solid var(--line); border-radius: 10px; padding: 12px 14px; text-align: left; display: flex; flex-direction: column; gap: 6px; width: 100%; transition: opacity .3s, border-color .35s, box-shadow .35s, background-color .35s; user-select: none; }
.card .id, .card .desc, .card .kind, .card .file, .card .lbl, .card .chip, .card .via { transition: color .35s, background-color .35s, opacity .3s; }
.kind { display: flex; align-items: center; gap: 6px; font-size: 11px; letter-spacing: .06em; text-transform: uppercase; color: var(--muted); }
.outcomes { display: flex; flex-direction: column; gap: 3px; margin-top: 6px; }
.outcome { font-size: 11px; color: var(--muted); border-left: 2px solid var(--line); padding-left: 6px; }
.outcome.fail { border-left-color: var(--bad); }
.kind .root { font-size: 10px; color: var(--good); border: 1px solid var(--good); border-radius: 4px; padding: 0 4px; letter-spacing: 0; }
.kind .chip { text-transform: none; letter-spacing: 0; margin-left: auto; }
.id { font-size: 13px; color: var(--text); word-break: break-all; }
.desc { color: var(--muted); font-size: 13px; }
.lbls { display: flex; flex-wrap: wrap; gap: 4px 8px; }
.lbl { font-size: 11px; color: var(--muted); }
.lbl.guard { color: var(--domain); } .lbl.fail { color: var(--bad); }
.g { font-size: 11px; margin-left: auto; }
.g.met { color: var(--good); } .g.unmet { color: var(--faint); }
.next.met { opacity: .95; border-color: var(--good); }
.next.unmet { opacity: .35; }
.next.unmet:hover, .next.unmet.hot { opacity: .8; }
.via { font-size: 11px; color: var(--api); border-top: 1px dashed var(--line); padding-top: 6px; margin-top: 2px; }
.past { opacity: .55; cursor: pointer; }
.past:hover { opacity: .9; }
.now { border-color: var(--now); box-shadow: 0 0 0 4px var(--now-glow), var(--shadow); cursor: default; }
.now .desc { color: var(--text); }
.meta { display: flex; flex-wrap: wrap; gap: 5px; margin-top: 2px; }
.file { font-size: 11px; color: var(--faint); }
.doc { font-size: 12px; color: var(--muted); border-top: 1px solid var(--line); padding-top: 6px; white-space: pre-wrap; }
.next { opacity: .6; cursor: pointer; }
.next:hover, .next.hot { opacity: 1; border-color: var(--text); }
.horizon { opacity: .28; pointer-events: none; }
.card.card.collapsed { opacity: 0; margin-top: -14px; padding-top: 0; padding-bottom: 0; border-top-width: 0; border-bottom-width: 0; border-color: transparent !important; pointer-events: none; }
.removed { border-style: dashed; }
.removed .id { text-decoration: line-through; text-decoration-color: var(--bad); }
.card.fade { animation: fade .28s ease-out both; }
@keyframes fade { from { opacity: 0; } }
</style>
