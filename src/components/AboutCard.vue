<script setup lang="ts">
import { NText } from "naive-ui";
import { t } from "../i18n";
import SectionCard from "./SectionCard.vue";

defineProps<{ version: string }>();
</script>

<template>
  <SectionCard :title="t.about.title">
    <p class="line">{{ t.about.heading(version) }}</p>
    <NText depth="3" tag="p" class="line">{{ t.about.license }}</NText>
    <NText depth="3" tag="p" class="line">{{ t.about.singBox }}</NText>
  </SectionCard>
</template>

<style scoped>
.line {
  margin: 0 0 4px;
}
</style>
