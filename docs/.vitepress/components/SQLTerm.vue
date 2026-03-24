<script setup>
import { ref, computed } from 'vue'

const props = defineProps({
  term: { type: String, required: true },
  definition: { type: String, default: '' }
})

const showTooltip = ref(false)
const anchor = computed(
  () => props.term.toLowerCase().replace(/\s+/g, '-')
)
</script>

<template>
  <span
    class="sql-term"
    @mouseenter="showTooltip = true"
    @mouseleave="showTooltip = false"
  >
    <a :href="`/ra/reference/sql-glossary.html#${anchor}`">
      <code>{{ term }}</code>
    </a>
    <span v-if="definition && showTooltip" class="sql-term-tooltip">
      {{ definition }}
    </span>
  </span>
</template>

<style scoped>
.sql-term {
  position: relative;
  display: inline;
}

.sql-term a {
  text-decoration: none;
  border-bottom: 1px dotted var(--vp-c-brand-1);
}

.sql-term a code {
  color: var(--vp-c-brand-1);
  font-size: 0.9em;
}

.sql-term a:hover code {
  color: var(--vp-c-brand-2);
}

.sql-term-tooltip {
  position: absolute;
  bottom: 100%;
  left: 50%;
  transform: translateX(-50%);
  background: var(--vp-c-bg-soft);
  border: 1px solid var(--vp-c-divider);
  border-radius: 6px;
  padding: 8px 12px;
  font-size: 0.85em;
  line-height: 1.4;
  color: var(--vp-c-text-1);
  white-space: normal;
  width: max-content;
  max-width: 300px;
  z-index: 100;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.15);
  pointer-events: none;
  margin-bottom: 4px;
}
</style>
