<script setup lang="ts">
import { computed, ref, useId } from "vue";
import { NButton, NFlex, NSwitch, NText, useMessage } from "naive-ui";
import type { AppState } from "../api";
import { t } from "../i18n";
import {
  actions,
  updateCancellable,
  updateChecking,
  updateFailure,
  updateInstalling,
  updateProgress,
} from "../store";
import { formatClock, formatUpdateProgress } from "../format";
import SectionCard from "./SectionCard.vue";

const props = defineProps<{ state: AppState }>();

const message = useMessage();
const headingId = useId();
const switchPending = ref(false);

/** A failed check, automatic or not, until the next one works; moot once a release is offered. */
const checkFailure = computed(() => {
  const last = props.state.lastCheck;
  if (!last?.error || props.state.update || updateChecking.value) return null;
  const sentence = /[。！？]$/.test(last.error) ? last.error : `${last.error}。`;
  return t.update.lastCheckFailed(formatClock(last.at), sentence);
});

async function setCheckUpdates(checkUpdates: boolean) {
  switchPending.value = true;
  if (await actions.updateSettings({ checkUpdates })) message.success(t.settings.saved);
  switchPending.value = false;
}

async function checkNow() {
  const result = await actions.checkUpdate();
  if (result === "latest") message.success(t.update.upToDate);
  else if (result) message.info(t.update.found(result.version));
}
</script>

<template>
  <SectionCard :title="t.update.title" :heading-id="headingId">
    <NFlex align="center" justify="space-between" :wrap="false">
      <div>
        <div>{{ t.update.autoCheck }}</div>
        <NText depth="3" class="hint">{{ t.update.autoCheckHint }}</NText>
      </div>
      <NSwitch
        :value="state.checkUpdates"
        :loading="switchPending"
        :aria-label="t.update.autoCheck"
        @update:value="setCheckUpdates"
      />
    </NFlex>

    <NFlex align="center" :size="8" class="row">
      <NText>{{ state.update ? t.update.available(state.update.version) : t.update.current(state.version) }}</NText>
      <NButton
        v-if="state.update"
        type="primary"
        size="small"
        :loading="updateInstalling"
        @click="actions.installUpdate"
      >
        {{ updateInstalling ? formatUpdateProgress(updateProgress) : updateFailure ? t.update.retry : t.update.install }}
      </NButton>
      <NButton v-else size="small" :loading="updateChecking" @click="checkNow">
        {{ updateChecking ? t.update.checking : t.update.checkNow }}
      </NButton>
      <NButton v-if="updateChecking || updateCancellable" size="small" @click="actions.cancelUpdate">
        {{ t.update.cancel }}
      </NButton>
      <NButton size="small" quaternary @click="actions.openReleases">
        {{ t.update.releases }}
      </NButton>
    </NFlex>
    <NText v-if="state.update && updateFailure" type="warning" tag="p" class="status">{{ updateFailure }}</NText>
    <NText v-else-if="checkFailure" depth="3" tag="p" class="status">
      {{ checkFailure }}<template v-if="state.checkUpdates">{{ t.update.retryLater }}</template>
    </NText>
  </SectionCard>
</template>

<style scoped>
.hint {
  display: block;
  font-size: 12px;
}

.row {
  margin-top: 14px;
}

.status {
  margin: 8px 0 0;
  font-size: 12px;
  word-break: break-word;
}
</style>
