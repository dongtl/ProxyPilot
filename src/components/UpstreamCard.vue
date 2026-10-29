<script setup lang="ts">
import { ref, useId, watch } from "vue";
import { NButton, NCard, NFlex, NText } from "naive-ui";
import type { AppState, ProbeReport, Upstream } from "../api";
import { t } from "../i18n";
import { actions } from "../store";
import { goTo } from "../navigation";
import { formatClient, formatUpstream } from "../format";
import UpstreamStatusTag from "./UpstreamStatusTag.vue";
import ProbeResult from "./ProbeResult.vue";

const props = defineProps<{ state: AppState; upstream: Upstream }>();

const headingId = useId();
const testing = ref(false);
const report = ref<ProbeReport | null>(null);

// A result for the previous upstream would be misleading.
watch(
  () => formatUpstream(props.upstream),
  () => {
    report.value = null;
  },
);

async function test() {
  testing.value = true;
  report.value = await actions.testUpstream();
  testing.value = false;
}
</script>

<template>
  <NCard tag="section" :aria-labelledby="headingId">
    <template #header>
      <h2 :id="headingId" class="pp-section-title">{{ t.upstream.title }}</h2>
    </template>
    <template #header-extra>
      <NButton size="small" quaternary :aria-label="t.upstream.changeAria" @click="goTo('settings')">
        {{ t.upstream.change }}
      </NButton>
    </template>

    <NFlex vertical :size="12">
      <NFlex align="center" justify="space-between">
        <div>
          <NFlex align="center" :size="8">
            <span class="client">{{ formatClient(state.upstreamClient, state.upstreamClientGuessed) }}</span>
            <UpstreamStatusTag :status="state.upstreamStatus" />
          </NFlex>
          <NText depth="3">{{ formatUpstream(upstream) }}</NText>
          <NText v-if="state.upstreamClientGuessed" depth="3" tag="p" class="pp-hint">{{ t.upstream.guessedHint }}</NText>
        </div>
        <NButton :loading="testing" @click="test">{{ t.upstream.test }}</NButton>
      </NFlex>

      <NText v-if="state.upstreamStatus === 'unreachable'" type="warning" tag="p" class="pp-hint">
        {{ t.upstream.unreachableHint }}
      </NText>
      <ProbeResult v-if="report" :report="report" />
    </NFlex>
  </NCard>
</template>

<style scoped>
.client {
  font-weight: 600;
}
</style>
