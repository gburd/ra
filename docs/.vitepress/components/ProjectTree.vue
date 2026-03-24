<script setup>
import { ref, computed } from 'vue'

const props = defineProps({
  data: {
    type: Array,
    required: true
  }
})

const searchQuery = ref('')
const expandedNodes = ref(new Set())

function nodeId(node, parentPath) {
  return parentPath ? `${parentPath}/${node.name}` : node.name
}

function toggleNode(id) {
  if (expandedNodes.value.has(id)) {
    expandedNodes.value.delete(id)
  } else {
    expandedNodes.value.add(id)
  }
}

function isExpanded(id) {
  return expandedNodes.value.has(id)
}

function expandAll(nodes, parentPath) {
  for (const node of nodes) {
    if (node.children) {
      const id = nodeId(node, parentPath)
      expandedNodes.value.add(id)
      expandAll(node.children, id)
    }
  }
}

function collapseAll() {
  expandedNodes.value.clear()
}

function nodeMatchesFilter(node, query, parentPath) {
  const lowerQuery = query.toLowerCase()
  if (node.name.toLowerCase().includes(lowerQuery)) {
    return true
  }
  if (node.desc && node.desc.toLowerCase().includes(lowerQuery)) {
    return true
  }
  if (node.children) {
    const id = nodeId(node, parentPath)
    return node.children.some(
      (child) => nodeMatchesFilter(child, query, id)
    )
  }
  return false
}

function filteredTree(nodes, parentPath) {
  if (!searchQuery.value) {
    return nodes
  }
  const result = []
  for (const node of nodes) {
    const id = nodeId(node, parentPath)
    if (nodeMatchesFilter(node, searchQuery.value, parentPath)) {
      if (node.children) {
        const filteredChildren = filteredTree(node.children, id)
        result.push({ ...node, children: filteredChildren })
        expandedNodes.value.add(id)
      } else {
        result.push(node)
      }
    }
  }
  return result
}

const visibleTree = computed(() => filteredTree(props.data, ''))

const categoryColors = {
  engine: '#e06c75',
  core: '#c678dd',
  parser: '#61afef',
  catalog: '#56b6c2',
  stats: '#98c379',
  hardware: '#d19a66',
  config: '#abb2bf',
  adapter: '#e5c07b',
  pg: '#336791',
  wasm: '#654ff0',
  ml: '#ff6f61',
  test: '#868e96',
  ui: '#20c997',
  tool: '#adb5bd',
  default: 'var(--vp-c-text-2)'
}

function categoryColor(cat) {
  return categoryColors[cat] || categoryColors.default
}
</script>

<template>
  <div class="project-tree">
    <div class="project-tree-controls">
      <input
        v-model="searchQuery"
        type="text"
        class="project-tree-search"
        placeholder="Filter by name or description..."
      />
      <div class="project-tree-buttons">
        <button
          class="project-tree-btn"
          @click="expandAll(props.data, '')"
        >Expand all</button>
        <button
          class="project-tree-btn"
          @click="collapseAll()"
        >Collapse all</button>
      </div>
    </div>
    <ul class="project-tree-root">
      <TreeNode
        v-for="node in visibleTree"
        :key="node.name"
        :node="node"
        :parent-path="''"
        :expanded-nodes="expandedNodes"
        :toggle="toggleNode"
        :is-expanded="isExpanded"
        :category-color="categoryColor"
      />
    </ul>
  </div>
</template>

<script>
import { defineComponent, h } from 'vue'

const TreeNode = defineComponent({
  name: 'TreeNode',
  props: {
    node: { type: Object, required: true },
    parentPath: { type: String, default: '' },
    expandedNodes: { type: Object, required: true },
    toggle: { type: Function, required: true },
    isExpanded: { type: Function, required: true },
    categoryColor: { type: Function, required: true }
  },
  setup(props) {
    function nodeId(node, parentPath) {
      return parentPath
        ? `${parentPath}/${node.name}`
        : node.name
    }

    return () => {
      const id = nodeId(props.node, props.parentPath)
      const isDir = Boolean(props.node.children)
      const expanded = props.isExpanded(id)
      const color = props.node.cat
        ? props.categoryColor(props.node.cat)
        : null

      const icon = isDir
        ? (expanded ? '\u{1F4C2}' : '\u{1F4C1}')
        : '\u{1F4C4}'

      const nameChildren = []

      nameChildren.push(
        h('span', { class: 'tree-icon' }, icon)
      )

      if (isDir) {
        nameChildren.push(
          h(
            'span',
            {
              class: 'tree-dir-name',
              style: color ? { color } : undefined,
              onClick: () => props.toggle(id)
            },
            props.node.name + '/'
          )
        )
      } else if (props.node.href) {
        nameChildren.push(
          h(
            'a',
            {
              class: 'tree-file-link',
              href: props.node.href,
              target: '_blank',
              rel: 'noopener noreferrer'
            },
            props.node.name
          )
        )
      } else if (props.node.anchor) {
        nameChildren.push(
          h(
            'a',
            {
              class: 'tree-file-link',
              href: props.node.anchor
            },
            props.node.name
          )
        )
      } else {
        nameChildren.push(
          h('span', { class: 'tree-file-name' }, props.node.name)
        )
      }

      if (props.node.desc) {
        nameChildren.push(
          h(
            'span',
            { class: 'tree-desc' },
            ' -- ' + props.node.desc
          )
        )
      }

      const liChildren = [
        h(
          'div',
          {
            class: [
              'tree-item',
              isDir ? 'tree-item-dir' : 'tree-item-file'
            ]
          },
          nameChildren
        )
      ]

      if (isDir && expanded && props.node.children.length > 0) {
        liChildren.push(
          h(
            'ul',
            { class: 'tree-children' },
            props.node.children.map((child) =>
              h(TreeNode, {
                key: child.name,
                node: child,
                parentPath: id,
                expandedNodes: props.expandedNodes,
                toggle: props.toggle,
                isExpanded: props.isExpanded,
                categoryColor: props.categoryColor
              })
            )
          )
        )
      }

      return h('li', { class: 'tree-node' }, liChildren)
    }
  }
})

export default {
  components: { TreeNode }
}
</script>

<style scoped>
.project-tree {
  font-family: var(--vp-font-family-mono);
  font-size: 0.85em;
  line-height: 1.6;
}

.project-tree-controls {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-bottom: 12px;
  align-items: center;
}

.project-tree-search {
  flex: 1;
  min-width: 200px;
  padding: 6px 10px;
  border: 1px solid var(--vp-c-divider);
  border-radius: 6px;
  background: var(--vp-c-bg-soft);
  color: var(--vp-c-text-1);
  font-size: 0.9em;
  font-family: var(--vp-font-family-base);
  outline: none;
}

.project-tree-search:focus {
  border-color: var(--vp-c-brand-1);
}

.project-tree-buttons {
  display: flex;
  gap: 4px;
}

.project-tree-btn {
  padding: 4px 10px;
  border: 1px solid var(--vp-c-divider);
  border-radius: 6px;
  background: var(--vp-c-bg-soft);
  color: var(--vp-c-text-2);
  font-size: 0.85em;
  cursor: pointer;
  transition: all 0.2s;
}

.project-tree-btn:hover {
  background: var(--vp-c-bg-alt);
  color: var(--vp-c-text-1);
  border-color: var(--vp-c-brand-1);
}

.project-tree-root {
  list-style: none;
  padding-left: 0;
  margin: 0;
}

.project-tree :deep(.tree-children) {
  list-style: none;
  padding-left: 20px;
  margin: 0;
  border-left: 1px solid var(--vp-c-divider);
}

.project-tree :deep(.tree-node) {
  margin: 0;
  padding: 0;
}

.project-tree :deep(.tree-item) {
  display: flex;
  align-items: baseline;
  padding: 1px 0;
  gap: 4px;
}

.project-tree :deep(.tree-item-dir) {
  cursor: pointer;
}

.project-tree :deep(.tree-icon) {
  flex-shrink: 0;
  width: 1.2em;
  text-align: center;
  font-style: normal;
}

.project-tree :deep(.tree-dir-name) {
  font-weight: 600;
  cursor: pointer;
}

.project-tree :deep(.tree-dir-name:hover) {
  text-decoration: underline;
}

.project-tree :deep(.tree-file-link) {
  color: var(--vp-c-brand-1);
  text-decoration: none;
}

.project-tree :deep(.tree-file-link:hover) {
  text-decoration: underline;
  color: var(--vp-c-brand-2);
}

.project-tree :deep(.tree-file-name) {
  color: var(--vp-c-text-1);
}

.project-tree :deep(.tree-desc) {
  color: var(--vp-c-text-3);
  font-family: var(--vp-font-family-base);
  font-size: 0.9em;
}
</style>
