<script setup lang="ts">
import { computed } from "vue";
import {
  NConfigProvider,
  NDialogProvider,
  NGlobalStyle,
  NMessageProvider,
  darkTheme,
  dateZhCN,
  useOsTheme,
  zhCN,
} from "naive-ui";
import AppShell from "./components/AppShell.vue";

const osTheme = useOsTheme();
const theme = computed(() => (osTheme.value === "dark" ? darkTheme : null));
</script>

<template>
  <NConfigProvider :theme="theme" :locale="zhCN" :date-locale="dateZhCN">
    <NGlobalStyle />
    <NMessageProvider>
      <NDialogProvider>
        <AppShell />
      </NDialogProvider>
    </NMessageProvider>
  </NConfigProvider>
</template>
