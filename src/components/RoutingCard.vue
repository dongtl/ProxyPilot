<script setup lang="ts">
import { computed, ref, useId } from "vue";
import { NCard, NFlex, NSwitch, NTag, NText } from "naive-ui";
import type { AppState } from "../api";
import { t } from "../i18n";
import { actions } from "../store";

const props = defineProps<{ state: AppState }>();

const headingId = useId();
const pending = ref(false);

type TagType = "success" | "warning" | "error" | "default";

const status = computed((): { type: TagType; label: string; detail: string } => {
  const { routingEnabled, coreStatus } = props.state;
  if (!routingEnabled) return { type: "default", label: t.routing.statusOff, detail: t.routing.detailOff };
  if (coreStatus === "running") return { type: "success", label: t.routing.statusOn, detail: t.routing.detailOn };
  if (coreStatus === "failed") return { type: "error", label: t.routing.statusFailed, detail: t.routing.detailFailed };
  // "starting", or "stopped" for the moment between the switch flipping and the core launching.
  return { type: "warning", label: t.routing.statusStarting, detail: t.routing.detailStarting };
});

// "running" with a message is a fallback to the previous rules; the findings list shows that one.
const showReason = computed(
  () => props.state.routingEnabled && props.state.coreStatus !== "running" && props.state.coreMessage !== "",
);

async function toggle(enabled: boolean) {
  pending.value = true;
  await actions.setRouting(enabled);
  pending.value = false;
}
</script>

<template>
  <NCard tag="section" :aria-labelledby="headingId">
    <NFlex align="center" justify="space-between" :wrap="false" :size="16">
      <div>
        <h2 :id="headingId" class="pp-section-title">{{ t.routing.title }}</h2>
        <div class="status" aria-live="polite">
          <NTag :type="status.type" :bordered="false">{{ status.label }}</NTag>
          <NText>{{ status.detail }}</NText>
        </div>
        <NText v-if="showReason" :type="state.coreStatus === 'failed' ? 'error' : 'warning'" tag="p" class="pp-hint">
          {{ t.routing.reasonPrefix }}{{ state.coreMessage }}
        </NText>
      </div>
      <NSwitch
        size="large"
        :value="state.routingEnabled"
        :loading="pending"
        :aria-labelledby="headingId"
        @update:value="toggle"
      >
        <template #checked>{{ t.routing.on }}</template>
        <template #unchecked>{{ t.routing.off }}</template>
      </NSwitch>
    </NFlex>
  </NCard>
</template>

<style scoped>
.status {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  margin-top: 8px;
}
</style>
