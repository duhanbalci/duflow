<script setup lang="ts">
import { computed } from 'vue'
import type { Candidate } from '../graph'
import { graph } from '../walk'

const props = defineProps<{
  id: string
  col: 'past' | 'now' | 'next' | 'horizon'
  cand?: Candidate
  hot?: boolean
}>()

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
</script>

<template>
  <component :is="col === 'now' ? 'div' : 'button'" class="card" :class="[col, { hot, removed: diffTag === 'removed', unmet: cand?.guard === 'false', met: cand?.guard === 'true' }]"
    :data-id="id" :data-layer="node?.layer" :data-cls="cand?.cls ?? ''" :data-guard="cand?.guard ?? ''" :data-labels="JSON.stringify(cand?.labels.map(l => ({ label: l.label, cls: l.cls })) ?? [])"
    :tabindex="col === 'now' ? -1 : 0">
    <div class="kind">
      <span class="dot"></span>{{ node?.kind }}
      <span v-if="isRoot" class="root">root</span>
      <span v-if="diffTag" class="chip" :class="diffTag">{{ diffTag === 'added' ? 'yeni' : diffTag === 'removed' ? 'artık yok' : 'değişti' }}</span>
    </div>
    <div class="id mono">{{ id }}</div>
    <div class="desc">{{ node?.desc }}</div>
    <div v-if="cand?.labels.length && col !== 'horizon'" class="lbls">
      <span v-for="l in cand.labels" :key="l.label" class="lbl" :class="l.cls">{{ l.label }}</span>
      <span v-if="cand.guard === 'true'" class="g met">✓ şu an sağlanıyor</span>
      <span v-else-if="cand.guard === 'false'" class="g unmet">✗ şu an sağlanmıyor</span>
    </div>

    <template v-if="col === 'now' && full">
      <div class="meta" v-if="Object.keys(full.attrs).length || full.checks.length || full.sets.length || listens.length">
        <span v-for="(v, k) in full.attrs" :key="k" class="chip attr mono">{{ k }} {{ v }}</span>
        <span v-for="c in full.checks" :key="c.name" class="chip check" :title="g?.checks.get(c.name)?.desc">
          check {{ c.name }}<template v-if="c.fail"> ✗ {{ c.fail }}</template>
        </span>
        <span v-for="s in full.sets" :key="s.var" class="chip set">sets {{ s.var }} {{ s.value }}</span>
        <span v-for="ev in listens" :key="ev" class="chip attr">dinler {{ ev }}</span>
      </div>
      <div class="file mono">{{ full.file }}</div>
      <div v-if="full.doc" class="doc">{{ full.doc }}</div>
    </template>

    <div v-if="cand?.via" class="via mono">
      via {{ cand.via.kind === 'call' ? (cand.via.attrs.method ?? '') + ' ' + (cand.via.attrs.path ?? cand.via.id) : cand.via.id }}
      <template v-if="cand.via.checks.length"> · {{ cand.via.checks.length }} check</template>
    </div>
  </component>
</template>

<style scoped>
.card { position: relative; background: var(--surface); border: 1px solid var(--line); border-radius: 10px; padding: 10px 12px; text-align: left; display: flex; flex-direction: column; gap: 6px; width: 100%; transition: opacity .22s, border-color .18s, box-shadow .18s; will-change: transform; }
.kind { display: flex; align-items: center; gap: 6px; font-size: 11px; letter-spacing: .06em; text-transform: uppercase; color: var(--muted); }
.kind .root { font-size: 10px; color: var(--good); border: 1px solid var(--good); border-radius: 4px; padding: 0 4px; letter-spacing: 0; }
.kind .chip { text-transform: none; letter-spacing: 0; margin-left: auto; }
.id { font-size: 12px; color: var(--text); word-break: break-all; }
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
.now { border-color: var(--now); box-shadow: 0 0 0 4px var(--now-glow), var(--shadow); padding: 14px 16px; gap: 8px; cursor: default; transition: opacity .22s, border-color .3s, box-shadow .3s, padding .3s; }
.now .id { font-size: 14px; }
.now .desc { color: var(--text); font-size: 14px; }
.meta { display: flex; flex-wrap: wrap; gap: 5px; margin-top: 2px; }
.file { font-size: 11px; color: var(--faint); }
.doc { font-size: 12px; color: var(--muted); border-top: 1px solid var(--line); padding-top: 6px; white-space: pre-wrap; }
.next { opacity: .6; cursor: pointer; }
.next:hover, .next.hot { opacity: 1; border-color: var(--text); }
.horizon { opacity: .28; pointer-events: none; }
.removed { border-style: dashed; }
.removed .id { text-decoration: line-through; text-decoration-color: var(--bad); }
.card.fade { animation: fade .28s ease-out both; }
@keyframes fade { from { opacity: 0; } }
</style>
