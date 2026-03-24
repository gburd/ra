<script setup>
import { computed } from 'vue'

const props = defineProps({
  expr: {
    type: String,
    required: true
  }
})

const operators = {
  sigma: { symbol: '\u03C3', name: 'Selection' },
  pi: { symbol: '\u03C0', name: 'Projection' },
  rho: { symbol: '\u03C1', name: 'Rename' },
  gamma: { symbol: '\u03B3', name: 'Aggregation' },
  delta: { symbol: '\u03B4', name: 'Duplicate Elimination' },
  tau: { symbol: '\u03C4', name: 'Sort' },
  join: { symbol: '\u22C8', name: 'Join' },
  semijoin: { symbol: '\u22C9', name: 'Semijoin' },
  antijoin: { symbol: '\u22CA', name: 'Antijoin' },
  leftjoin: { symbol: '\u27D5', name: 'Left Outer Join' },
  rightjoin: { symbol: '\u27D6', name: 'Right Outer Join' },
  fulljoin: { symbol: '\u27D7', name: 'Full Outer Join' },
  union: { symbol: '\u222A', name: 'Union' },
  intersect: { symbol: '\u2229', name: 'Intersection' },
  except: { symbol: '\u2212', name: 'Difference' },
  cross: { symbol: '\u00D7', name: 'Cartesian Product' },
  divide: { symbol: '\u00F7', name: 'Division' },
  natural: { symbol: '\u22C8', name: 'Natural Join' }
}

function convertExpr(text) {
  const segments = []
  let remaining = text

  while (remaining.length > 0) {
    let matched = false

    for (const [keyword, op] of Object.entries(operators)) {
      const pattern = new RegExp(`^${keyword}(?=\\[|\\()`)
      const infixPattern = new RegExp(
        `^\\s+${keyword}(?:\\[([^\\]]+)\\])?\\s+`
      )
      const match = remaining.match(pattern)
      const infixMatch = remaining.match(infixPattern)

      if (match) {
        segments.push({
          type: 'operator',
          symbol: op.symbol,
          name: op.name
        })
        remaining = remaining.slice(match[0].length)
        matched = true
        break
      }

      if (infixMatch) {
        segments.push({ type: 'text', value: ' ' })
        segments.push({
          type: 'operator',
          symbol: op.symbol,
          name: op.name
        })
        if (infixMatch[1]) {
          segments.push({
            type: 'subscript',
            value: infixMatch[1]
          })
        }
        segments.push({ type: 'text', value: ' ' })
        remaining = remaining.slice(infixMatch[0].length)
        matched = true
        break
      }
    }

    if (!matched) {
      const lastSeg = segments[segments.length - 1]
      if (lastSeg && lastSeg.type === 'text') {
        lastSeg.value += remaining[0]
      } else {
        segments.push({ type: 'text', value: remaining[0] })
      }
      remaining = remaining.slice(1)
    }
  }

  return segments
}

const parsed = computed(() => convertExpr(props.expr))
</script>

<template>
  <span class="rel-algebra">
    <template v-for="(seg, i) in parsed" :key="i">
      <span
        v-if="seg.type === 'operator'"
        class="ra-op"
        :title="seg.name"
      >{{ seg.symbol }}</span>
      <sub
        v-else-if="seg.type === 'subscript'"
        class="ra-sub"
      >{{ seg.value }}</sub>
      <span v-else>{{ seg.value }}</span>
    </template>
  </span>
</template>

<style scoped>
.rel-algebra {
  font-family: 'KaTeX_Main', 'Times New Roman', serif;
  font-size: 1.1em;
  white-space: nowrap;
}
.ra-op {
  color: var(--vp-c-brand-1);
  cursor: help;
  font-weight: 500;
}
.ra-sub {
  font-size: 0.75em;
  color: var(--vp-c-text-2);
}
</style>
