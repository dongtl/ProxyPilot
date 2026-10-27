<script setup lang="ts">
import { computed } from "vue";
import { NAlert, NText } from "naive-ui";
import type { ProbeReport } from "../api";
import { t } from "../i18n";

const props = defineProps<{ report: ProbeReport }>();

const stages = computed(() =>
  props.report.stages.map((s) => t.upstream.stageMs(t.upstream.stage[s.name] ?? s.name, s.ms)).join(" · "),
);
</script>

<template>
  <NAlert :type="report.ok ? 'success' : 'error'" :title="report.ok ? t.upstream.testOk : t.upstream.testFailed">
    <div>{{ report.message }}</div>
    <NText depth="3" tag="div" class="pp-hint">
      {{ t.upstream.testSummary(report.target, report.elapsedMs) }}
    </NText>
    <NText v-if="stages" depth="3" tag="div">{{ stages }}</NText>
  </NAlert>
</template>
