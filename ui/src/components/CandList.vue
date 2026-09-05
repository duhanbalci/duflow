<script setup lang="ts">
// Aday sütunu içeriği: ≤6 düz liste, üstü gruplu (+süzgeç). Hem "next" hem "incoming" slotunda kullanılır.
import { computed } from 'vue'
import Card from './Card.vue'
import { groupOf, type Candidate } from '../graph'
import { graph, state } from '../walk'

const props = defineProps<{ cands: Candidate[]; hot?: string; interactive?: boolean; selected?: string }>()

const grouped = computed(() => {
  const ns = props.cands
  const filter = props.interactive ? state.filter : ''
  if (ns.length <= 6 && !filter) return null
  const groups = new Map<string, Candidate[]>()
  for (const c of ns) {
    const gk = c.diff === 'removed' ? 'artık yok' : c.cls === 'fail' ? 'hata dalları' : groupOf(c.to)
    if (!groups.has(gk)) groups.set(gk, [])
    groups.get(gk)!.push(c)
  }
  const f = filter.toLowerCase()
  const out: { name: string; list: Candidate[]; open: boolean; fail: boolean }[] = []
  for (const [name, list0] of groups) {
    const list = f ? list0.filter((c) => c.to.includes(f) || (graph.value?.get(c.to)?.desc ?? '').toLowerCase().includes(f)) : list0
    if (!list.length) continue
    const open = props.interactive ? state.openGroups.has(name) || !!f : list.some((c) => c.to === props.selected)
    out.push({ name, list, open, fail: name === 'hata dalları' || name === 'artık yok' })
  }
  out.sort((a, b) => Number(a.fail) - Number(b.fail))
  return out
})

function toggleGroup(name: string) {
  if (!props.interactive) return
  const s = new Set(state.openGroups)
  if (s.has(name)) s.delete(name); else s.add(name)
  state.openGroups = s
}
const key = (c: Candidate) => c.to + (c.via?.id ?? '')
</script>

<template>
  <div v-if="!cands.length" class="empty">çıkış yok</div>
  <template v-else-if="!grouped">
    <Card v-for="c in cands" :key="key(c)" :id="c.to" :col="selected === c.to ? 'now' : 'next'" :cand="selected === c.to ? undefined : c" :hot="hot === c.to" :class="{ dropped: selected && selected !== c.to }" />
  </template>
  <template v-else>
    <input v-if="interactive" class="nfilter mono" :placeholder="`${cands.length} çıkış · süz…`" v-model="state.filter" />
    <template v-for="grp in grouped" :key="grp.name">
      <button class="ghead" :class="{ fail: grp.fail, dropped: !!selected }" @click.stop="toggleGroup(grp.name)">
        <span class="mono">{{ grp.name }}</span><span>{{ grp.list.length }}</span>
      </button>
      <Card v-for="c in (grp.open ? grp.list : grp.list.slice(0, 2))" :key="key(c)" :id="c.to" :col="selected === c.to ? 'now' : 'next'" :cand="selected === c.to ? undefined : c" :hot="hot === c.to" :class="{ dropped: selected && selected !== c.to }" />
      <button v-if="!grp.open && grp.list.length > 2" class="more" :class="{ dropped: !!selected }" @click.stop="toggleGroup(grp.name)">+{{ grp.list.length - 2 }} daha</button>
    </template>
  </template>
</template>

<style scoped>
.empty { color: var(--faint); font-size: 12px; text-align: center; padding: 20px 8px; border: 1px dashed var(--line); border-radius: 10px; }
.nfilter { background: var(--surface2); border: 1px solid var(--line); border-radius: 6px; padding: 5px 8px; font-size: 12px; outline: 0; }
.nfilter:focus { border-color: var(--text); }
.ghead { display: flex; justify-content: space-between; font-size: 11px; letter-spacing: .06em; text-transform: uppercase; color: var(--muted); padding: 6px 2px 0; margin-top: 4px; border-top: 1px solid var(--line); }
.ghead.fail { color: var(--bad); }
.ghead:hover { color: var(--text); }
.more { font-size: 12px; color: var(--faint); text-align: left; padding: 2px 12px; }
.more:hover { color: var(--text); }
.dropped { opacity: 0 !important; transition: opacity .25s; pointer-events: none; }
</style>
