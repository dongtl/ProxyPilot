<script setup lang="ts">
// Shown above every page while a newer release is waiting.
import { NAlert, NButton, NFlex, NText } from "naive-ui";
import type { UpdateInfo } from "../api";
import { t } from "../i18n";
import { actions, updateInstalling, updateProgress } from "../store";
import { formatUpdateProgress } from "../format";

defineProps<{ update: UpdateInfo }>();
</script>

<template>
  <NAlert type="info" :title="t.update.bannerTitle(update.version)" class="banner">
    <pre v-if="update.notes" class="notes">{{ update.notes }}</pre>
    <NText depth="3" tag="p" class="note">{{ t.update.restartNote }}</NText>
    <NFlex :size="8" align="center">
      <NButton type="primary" size="small" :loading="updateInstalling" @click="actions.installUpdate">
        {{ updateInstalling ? formatUpdateProgress(updateProgress) : t.update.install }}
      </NButton>
      <NButton size="small" :disabled="updateInstalling" @click="actions.openReleases">{{ t.update.details }}</NButton>
      <NButton
        size="small"
        quaternary
        :disabled="updateInstalling"
        @click="actions.dismissUpdate(update.version)"
      >
        {{ t.update.later }}
      </NButton>
    </NFlex>
    <p class="sr-only" aria-live="polite">{{ updateInstalling ? formatUpdateProgress(updateProgress) : "" }}</p>
  </NAlert>
</template>

<style scoped>
.banner {
  margin-bottom: 16px;
}

.notes {
  margin: 0 0 8px;
  max-height: 120px;
  overflow-y: auto;
  font-family: inherit;
  white-space: pre-wrap;
  word-break: break-word;
}

.note {
  margin: 0 0 10px;
  font-size: 12px;
}
</style>
