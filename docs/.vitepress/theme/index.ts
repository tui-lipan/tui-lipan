import DefaultTheme from 'vitepress/theme'
import { h } from 'vue'

import NavGitHubLink from './NavGitHubLink.vue'
import NavTitleMeta from './NavTitleMeta.vue'
import './style.css'

export default {
  extends: DefaultTheme,
  Layout: () => {
    return h(DefaultTheme.Layout, null, {
      'nav-bar-title-after': () => h(NavTitleMeta),
      'nav-bar-content-after': () => h(NavGitHubLink),
    })
  },
}
