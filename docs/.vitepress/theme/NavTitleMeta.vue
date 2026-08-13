<script setup lang="ts">
// Comes from Cargo.toml via config.ts, so the site cannot advertise a release
// that was never cut.
import { useData } from "vitepress";

const { theme } = useData();
</script>

<template>
  <span class="NavTitleLabel">docs</span>
  <span class="NavTitleVersion">v{{ theme.lipanVersion }}</span>
</template>
