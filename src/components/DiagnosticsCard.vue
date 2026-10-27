<script setup lang="ts">
// One-click self-check for people who cannot read logs: run it, copy the
// report, paste it to whoever is helping them.
import { computed, ref, useId } from "vue";
import { NAlert, NButton, NFlex, NInput, NTag, NText, useMessage } from "naive-ui";
import type { AppState, CheckStatus, DiagnosticReport } from "../api";
import { t } from "../i18n";
import { actions } from "../store";
import SectionCard from "./SectionCard.vue";

const props = defineProps<{ state: AppState }>();

const message = useMessage();
const headingId = useId();
const inputId = useId();
const intranet = ref("");
const running = ref(false);
const report = ref<DiagnosticReport | null>(null);

const routingActive = computed(() => props.state.routingEnabled && props.state.coreStatus === "running");

const tagType: Record<CheckStatus, "success" | "warning" | "error" | "default"> = {
  ok: "success",
  warn: "warning",
  fail: "error",
  skip: "default",
};

const failed = computed(() => report.value?.items.filter((item) => item.status === "fail").length ?? 0);

async function run() {
  running.value = true;
  report.value = (await actions.runDiagnostics(intranet.value.trim() || null)) ?? report.value;
  running.value = false;
}

async function copy() {
  if (!report.value) return;
  try {
    await navigator.clipboard.writeText(report.value.text);
    message.success(t.diagnostics.copied);
  } catch {
    message.warning(t.diagnostics.copyFailed, { duration: 8000 });
  }
}
</script>

<template>
  <SectionCard :title="t.diagnostics.title" :heading-id="headingId">
    <NText depth="3" tag="p" class="intro">{{ t.diagnostics.intro }}</NText>
    <NAlert v-if="!routingActive" type="info" class="notice">{{ t.diagnostics.needsRouting }}</NAlert>

    <label :for="inputId" class="label">{{ t.diagnostics.intranetLabel }}</label>
    <NFlex :wrap="false" :size="8">
      <NInput
        v-model:value="intranet"
        clearable
        :placeholder="t.diagnostics.intranetPlaceholder"
        :input-props="{ id: inputId }"
        :disabled="running"
        @keyup.enter="run"
      />
      <NButton type="primary" :loading="running" @click="run">{{ t.diagnostics.run }}</NButton>
    </NFlex>
    <NText depth="3" tag="p" class="pp-hint">{{ t.diagnostics.intranetHint }}</NText>

    <div aria-live="polite">
      <NText v-if="running" tag="p">{{ t.diagnostics.running }}</NText>
      <template v-else-if="report">
        <NText tag="p" :type="failed ? 'error' : 'success'" class="summary">
          {{ failed ? t.diagnostics.someFailed(failed) : t.diagnostics.allOk }}
        </NText>
        <ul class="items">
          <li v-for="item in report.items" :key="item.id">
            <NTag size="small" :type="tagType[item.status]" :bordered="false">{{ t.diagnostics.status[item.status] }}</NTag>
            <span class="title">{{ item.title }}</span>
            <NText depth="3">{{ item.detail }}</NText>
          </li>
        </ul>
        <NFlex :size="8">
          <NButton @click="copy">{{ t.diagnostics.copy }}</NButton>
          <NButton quaternary @click="actions.openDataDir">{{ t.diagnostics.openFile }}</NButton>
        </NFlex>
      </template>
    </div>
  </SectionCard>
</template>

<style scoped>
.intro {
  margin: 0 0 12px;
}

.notice {
  margin-bottom: 12px;
}

.label {
  display: block;
  margin-bottom: 6px;
}

.summary {
  font-weight: 600;
  margin: 12px 0 8px;
}

.items {
  list-style: none;
  padding: 0;
  margin: 0 0 12px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.items li {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  gap: 8px;
}

.title {
  font-weight: 600;
}
</style>
