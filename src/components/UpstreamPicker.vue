<script setup lang="ts">
// Finds local proxy ports and lets the user pick one (or type one in).
// Used for first run on Home and for "更换" in Settings.
import { computed, onMounted, ref } from "vue";
import { NButton, NFlex, NList, NListItem, NSpin, NTag, NText } from "naive-ui";
import type { Candidate, Upstream } from "../api";
import { t } from "../i18n";
import { actions, appState } from "../store";
import { formatEndpoint } from "../format";
import ManualUpstreamForm from "./ManualUpstreamForm.vue";

const props = defineProps<{ autoDetect?: boolean }>();
const emit = defineEmits<{ picked: [upstream: Upstream] }>();

const candidates = ref<Candidate[]>([]);
const scanned = ref(false);
const scanning = ref(false);
const pendingKey = ref<string | null>(null);
const showManual = ref(false);

const usableCount = computed(() => candidates.value.filter((c) => c.status === "usable").length);

const statusText = computed(() => {
  if (!scanned.value) return t.picker.notScanned;
  if (usableCount.value === 0) return t.picker.noneFound;
  if (usableCount.value === 1) return t.picker.foundOne;
  return t.picker.found(usableCount.value);
});

function keyOf(c: Candidate): string {
  return `${c.host}:${c.port}`;
}

function isCurrent(c: Candidate): boolean {
  const current = appState.value?.upstream;
  return !!current && current.kind === "socks5" && current.host === c.host && current.port === c.port;
}

async function detect(full: boolean) {
  scanning.value = true;
  const found = await actions.detectUpstreams(full);
  if (found) {
    candidates.value = found;
    scanned.value = true;
  }
  scanning.value = false;
}

async function pick(upstream: Upstream, key: string) {
  pendingKey.value = key;
  const ok = await actions.setUpstream(upstream);
  pendingKey.value = null;
  if (ok) emit("picked", upstream);
}

function pickCandidate(c: Candidate) {
  return pick({ kind: "socks5", host: c.host, port: c.port }, keyOf(c));
}

function pickManual(upstream: Upstream) {
  return pick(upstream, "manual");
}

onMounted(() => {
  if (props.autoDetect) void detect(false);
});
</script>

<template>
  <NFlex vertical :size="12">
    <div aria-live="polite">
      <NFlex v-if="scanning" align="center" :size="8">
        <NSpin size="small" />
        <NText>{{ t.picker.detecting }}</NText>
      </NFlex>
      <NText v-else>{{ statusText }}</NText>
    </div>

    <NList v-if="!scanning && candidates.length" bordered>
      <NListItem v-for="c in candidates" :key="keyOf(c)">
        <NFlex align="center" :size="8">
          <span class="client">{{ c.client }}</span>
          <NTag v-if="c.known" size="small" type="info" :bordered="false">{{ t.picker.known }}</NTag>
          <NTag v-if="c.status === 'needsAuth'" size="small" type="warning" :bordered="false">
            {{ t.picker.needsAuth }}
          </NTag>
        </NFlex>
        <NText depth="3">{{ t.picker.candidateDetail(formatEndpoint(c.host, c.port), c.process) }}</NText>
        <template #suffix>
          <NTag v-if="isCurrent(c)" type="success" :bordered="false">{{ t.picker.inUse }}</NTag>
          <NButton
            v-else-if="c.status === 'usable'"
            :type="usableCount === 1 ? 'primary' : 'default'"
            :loading="pendingKey === keyOf(c)"
            :aria-label="t.picker.useAria(c.client, c.port)"
            @click="pickCandidate(c)"
          >
            {{ t.picker.use }}
          </NButton>
        </template>
      </NListItem>
    </NList>

    <NFlex :size="8">
      <NButton :loading="scanning" @click="detect(false)">
        {{ scanned ? t.picker.redetect : t.picker.detect }}
      </NButton>
      <NButton :loading="scanning" @click="detect(true)">{{ t.picker.fullScan }}</NButton>
      <NButton quaternary :aria-expanded="showManual" @click="showManual = !showManual">
        {{ showManual ? t.picker.manualHide : t.picker.manualToggle }}
      </NButton>
    </NFlex>
    <NText depth="3" class="hint">{{ t.picker.fullScanHint }}</NText>

    <ManualUpstreamForm v-if="showManual" :loading="pendingKey === 'manual'" @submit="pickManual" />
  </NFlex>
</template>

<style scoped>
.client {
  font-weight: 600;
}

.hint {
  font-size: 12px;
}
</style>
