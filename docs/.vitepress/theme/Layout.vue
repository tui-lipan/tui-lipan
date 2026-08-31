<script setup lang="ts">
import DefaultTheme from "vitepress/theme";
import NavGitHubLink from "./NavGitHubLink.vue";
import NavTitleMeta from "./NavTitleMeta.vue";

const { Layout: DefaultLayout } = DefaultTheme;

/**
 * Seven marks down the right band, four down the left; `r6` and `l3` are the
 * prompt glyph out of the logo's middle rather than the whole thing. Where
 * each one sits and how large it is lives in `.doc-bg` in style.css, beside
 * the mask that trims them to the free space either side of the reading
 * column.
 */
const MARKS = [
  "r1",
  "r2",
  "r3",
  "r4",
  "r5",
  "r6",
  "r7",
  "l1",
  "l2",
  "l3",
  "l4",
];
</script>

<template>
  <!-- The landing page's mark, quieter and kept out of the reading column -
       see `.doc-bg` in style.css. Fixed, so it costs no layout. -->
  <div class="doc-bg" aria-hidden="true">
    <span v-for="mark in MARKS" :key="mark" :class="mark"></span>
  </div>
  <DefaultLayout>
    <template #nav-bar-title-after><NavTitleMeta /></template>
    <template #nav-bar-content-after><NavGitHubLink /></template>
    <template #nav-screen-content-after><NavGitHubLink /></template>
  </DefaultLayout>
</template>
