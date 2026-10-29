<script setup lang="ts">
import { ref } from "vue";
import { NButton, NFlex, NText, useMessage } from "naive-ui";
import type { AppState } from "../api";
import { t } from "../i18n";
import { formatClient, formatUpstream } from "../format";
import SectionCard from "./SectionCard.vue";
import UpstreamPicker from "./UpstreamPicker.vue";
import UpstreamStatusTag from "./UpstreamStatusTag.vue";

defineProps<{ state: AppState }>();

const message = useMessage();
const changing = ref(false);

function onPicked() {
  changing.value = false;
  message.success(t.upstream.changed);
}
</script>

<template>
  <SectionCard :title="t.settings.upstreamTitle">
    <template #extra>
      <NButton size="small" :aria-expanded="changing" @click="changing = !changing">
        {{ changing ? t.upstream.changeCancel : t.upstream.change }}
      </NButton>
    </template>

    <NFlex vertical :size="12">
      <div v-if="state.upstream">
        <NFlex align="center" :size="8">
          <span class="client">{{ formatClient(state.upstreamClient, state.upstreamClientGuessed) }}</span>
          <UpstreamStatusTag :status="state.upstreamStatus" />
        </NFlex>
        <NText depth="3">{{ formatUpstream(state.upstream) }}</NText>
        <NText v-if="state.upstreamClientGuessed" depth="3" tag="p" class="pp-hint">{{ t.upstream.guessedHint }}</NText>
      </div>
      <NText v-else depth="3">{{ t.upstream.none }}</NText>

      <UpstreamPicker v-if="changing" auto-detect @picked="onPicked" />
    </NFlex>
  </SectionCard>
</template>

<style scoped>
.client {
  font-weight: 600;
}
</style>
