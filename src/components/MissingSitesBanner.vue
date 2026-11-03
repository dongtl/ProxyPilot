<script setup lang="ts">
// Sites a proxied page needed but could not reach directly, one alert per
// item. Nothing changes until the user picks "一起走代理".
import { ref } from "vue";
import { NAlert, NButton, NFlex, useMessage } from "naive-ui";
import type { Suggestion } from "../api";
import { t } from "../i18n";
import { actions } from "../store";

defineProps<{ suggestions: Suggestion[] }>();

const message = useMessage();
const pending = ref<string | null>(null);

const listOf = (sites: string[]) => sites.join(t.common.listSeparator);

async function act(suggestion: Suggestion, what: "accept" | "ignore" | "dismiss") {
  if (pending.value) return;
  pending.value = suggestion.profileId;
  if (what === "accept") {
    const sites = await actions.acceptSuggestion(suggestion.profileId);
    if (sites?.length) message.success(t.missing.accepted(listOf(sites)), { duration: 6000 });
  } else if (what === "ignore") {
    await actions.ignoreSuggestion(suggestion.profileId);
  } else {
    await actions.dismissSuggestion(suggestion.profileId);
  }
  pending.value = null;
}
</script>

<template>
  <NAlert
    v-for="suggestion in suggestions"
    :key="suggestion.profileId"
    type="warning"
    :title="t.missing.title(suggestion.profileName)"
    class="banner"
  >
    <p class="body">{{ t.missing.body(listOf(suggestion.sites)) }}</p>
    <NFlex :size="8" align="center">
      <NButton
        type="primary"
        size="small"
        :loading="pending === suggestion.profileId"
        @click="act(suggestion, 'accept')"
      >
        {{ t.missing.accept }}
      </NButton>
      <NButton size="small" :disabled="pending === suggestion.profileId" @click="act(suggestion, 'ignore')">
        {{ t.missing.ignore }}
      </NButton>
      <NButton
        size="small"
        quaternary
        :disabled="pending === suggestion.profileId"
        @click="act(suggestion, 'dismiss')"
      >
        {{ t.missing.dismiss }}
      </NButton>
    </NFlex>
  </NAlert>
</template>

<style scoped>
.banner {
  margin-bottom: 16px;
}

.body {
  margin: 0 0 10px;
  word-break: break-word;
}
</style>
