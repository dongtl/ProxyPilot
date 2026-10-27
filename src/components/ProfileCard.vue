<script setup lang="ts">
import { computed, ref, useId } from "vue";
import { NButton, NCard, NFlex, NSwitch, NText, useDialog, useMessage } from "naive-ui";
import type { Profile } from "../api";
import { t } from "../i18n";
import { actions } from "../store";
import { summarizeList } from "../format";

const props = defineProps<{ profile: Profile }>();

const PREVIEW = 4;

const dialog = useDialog();
const message = useMessage();
const headingId = useId();
const toggling = ref(false);
const expanded = ref(false);

const canExpand = computed(
  () => props.profile.domains.length > PREVIEW || props.profile.processes.length > PREVIEW,
);
const limit = computed(() => (expanded.value ? Infinity : PREVIEW));

async function toggle(enabled: boolean) {
  toggling.value = true;
  await actions.setProfileEnabled(props.profile.id, enabled);
  toggling.value = false;
}

function confirmRemove() {
  dialog.warning({
    title: t.profiles.deleteTitle,
    content: t.profiles.deleteBody(props.profile.name),
    positiveText: t.common.delete,
    negativeText: t.common.cancel,
    onPositiveClick: async () => {
      if (await actions.removeProfile(props.profile.id)) message.success(t.profiles.deleted);
    },
  });
}
</script>

<template>
  <NCard tag="article" size="small" embedded :aria-labelledby="headingId">
    <template #header>
      <h3 :id="headingId" class="pp-item-title">{{ profile.name }}</h3>
    </template>
    <template #header-extra>
      <NFlex align="center" :size="8" :wrap="false">
        <NText depth="3" class="state">{{ profile.enabled ? t.profiles.enabled : t.profiles.disabled }}</NText>
        <NSwitch
          :value="profile.enabled"
          :loading="toggling"
          :aria-label="t.profiles.switchAria(profile.name)"
          @update:value="toggle"
        />
      </NFlex>
    </template>

    <NText v-if="profile.description" tag="p" class="desc">{{ profile.description }}</NText>
    <NText v-if="profile.domains.length" depth="3" tag="div" class="list">
      {{ t.profiles.domains }}{{ summarizeList(profile.domains, limit) }}
    </NText>
    <NText v-if="profile.processes.length" depth="3" tag="div" class="list">
      {{ t.profiles.processes }}{{ summarizeList(profile.processes, limit) }}
    </NText>

    <template v-if="canExpand || profile.source === 'custom'" #action>
      <NFlex justify="space-between" align="center">
        <NButton v-if="canExpand" text size="small" :aria-expanded="expanded" @click="expanded = !expanded">
          {{ expanded ? t.common.showLess : t.common.showAll }}
        </NButton>
        <span v-else />
        <NButton
          v-if="profile.source === 'custom'"
          size="small"
          quaternary
          type="error"
          :aria-label="t.profiles.deleteAria(profile.name)"
          @click="confirmRemove"
        >
          {{ t.common.delete }}
        </NButton>
      </NFlex>
    </template>
  </NCard>
</template>

<style scoped>
.state {
  font-size: 12px;
}

.desc {
  margin: 0 0 6px;
}

.list {
  font-size: 13px;
  word-break: break-all;
}
</style>
