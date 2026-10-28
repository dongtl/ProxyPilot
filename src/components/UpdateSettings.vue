<script setup lang="ts">
import { ref, useId } from "vue";
import { NButton, NFlex, NSwitch, NText, useMessage } from "naive-ui";
import type { AppState } from "../api";
import { t } from "../i18n";
import { actions, updateInstalling, updateProgress } from "../store";
import { formatUpdateProgress } from "../format";
import SectionCard from "./SectionCard.vue";

defineProps<{ state: AppState }>();

const message = useMessage();
const headingId = useId();
const switchPending = ref(false);
const checking = ref(false);

async function setCheckUpdates(checkUpdates: boolean) {
  switchPending.value = true;
  if (await actions.updateSettings({ checkUpdates })) message.success(t.settings.saved);
  switchPending.value = false;
}

async function checkNow() {
  checking.value = true;
  const result = await actions.checkUpdate();
  checking.value = false;
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
        {{ updateInstalling ? formatUpdateProgress(updateProgress) : t.update.install }}
      </NButton>
      <NButton v-else size="small" :loading="checking" @click="checkNow">{{ t.update.checkNow }}</NButton>
      <NButton size="small" quaternary :disabled="updateInstalling" @click="actions.openReleases">
        {{ t.update.releases }}
      </NButton>
    </NFlex>
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
</style>
