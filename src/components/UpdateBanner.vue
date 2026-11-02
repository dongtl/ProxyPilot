<script setup lang="ts">
// Shown above every page while a newer release is waiting. Downloading can be
// cancelled; a failure stays here with its reason until the next try.
import { NAlert, NButton, NFlex, NText } from "naive-ui";
import type { UpdateInfo } from "../api";
import { t } from "../i18n";
import { actions, updateCancellable, updateFailure, updateInstalling, updateProgress } from "../store";
import { formatUpdateProgress } from "../format";

defineProps<{ update: UpdateInfo }>();
</script>

<template>
  <NAlert
    :type="updateFailure ? 'warning' : 'info'"
    :title="updateFailure ? t.update.failedTitle(update.version) : t.update.bannerTitle(update.version)"
    class="banner"
  >
    <p v-if="updateFailure" class="failure">{{ updateFailure }}</p>
    <template v-else>
      <pre v-if="update.notes" class="notes">{{ update.notes }}</pre>
      <NText depth="3" tag="p" class="note">{{ t.update.restartNote }}</NText>
    </template>
    <NFlex :size="8" align="center">
      <NButton type="primary" size="small" :loading="updateInstalling" @click="actions.installUpdate">
        {{ updateInstalling ? formatUpdateProgress(updateProgress) : updateFailure ? t.update.retry : t.update.install }}
      </NButton>
      <NButton v-if="updateCancellable" size="small" @click="actions.cancelUpdate">{{ t.update.cancel }}</NButton>
      <NButton size="small" @click="actions.openReleases">
        {{ updateFailure ? t.update.manual : t.update.details }}
      </NButton>
      <NButton v-if="!updateInstalling" size="small" quaternary @click="actions.dismissUpdate(update.version)">
        {{ t.update.later }}
      </NButton>
    </NFlex>
    <p class="sr-only" aria-live="polite">
      {{ updateInstalling ? formatUpdateProgress(updateProgress) : (updateFailure ?? "") }}
    </p>
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

.failure {
  margin: 0 0 10px;
  word-break: break-word;
}
</style>
