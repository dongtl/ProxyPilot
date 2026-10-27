<script setup lang="ts">
import { computed } from "vue";
import { NAlert, NFlex, NText } from "naive-ui";
import type { EnvFinding } from "../api";
import { t } from "../i18n";
import SectionCard from "./SectionCard.vue";

const props = defineProps<{ findings: EnvFinding[] }>();

const order: Record<EnvFinding["level"], number> = { error: 0, warning: 1, info: 2 };

const sorted = computed(() => [...props.findings].sort((a, b) => order[a.level] - order[b.level]));
</script>

<template>
  <SectionCard :title="t.env.title">
    <NFlex v-if="sorted.length" vertical :size="8">
      <NAlert v-for="finding in sorted" :key="finding.id" :type="finding.level">
        <template #header>{{ t.env.heading(t.env.level[finding.level], finding.title) }}</template>
        {{ finding.detail }}
      </NAlert>
    </NFlex>
    <NText v-else depth="3">{{ t.env.allGood }}</NText>
  </SectionCard>
</template>
