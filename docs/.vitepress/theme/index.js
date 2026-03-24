import DefaultTheme from 'vitepress/theme'
import 'katex/dist/katex.min.css'
import './custom.css'
import RelAlgebra from '../components/RelAlgebra.vue'
import SQLTerm from '../components/SQLTerm.vue'
import ProjectTree from '../components/ProjectTree.vue'

export default {
  extends: DefaultTheme,
  enhanceApp({ app }) {
    app.component('RelAlgebra', RelAlgebra)
    app.component('SQLTerm', SQLTerm)
    app.component('ProjectTree', ProjectTree)
  }
}
