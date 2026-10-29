<script setup lang="ts">
import { computed, ref } from "vue";
import { NButton, NSwitch, NTag, NText, useDialog, useMessage, useThemeVars } from "naive-ui";
import type { Profile } from "../api";
import { t } from "../i18n";
import { actions, isMac } from "../store";
import { coverageLines, stillProxied } from "../coverage";

// One compact row per site or app the user added: they hold one entry each,
// and a card apiece turned a few dozen of them into a long wall of boxes.
const props = defineProps<{ profiles: Profile[]; all: Profile[] }>();

const dialog = useDialog();
const message = useMessage();
const themeVars = useThemeVars();
const toggling = ref<string | null>(null);

// Newest first, so what was just added sits right under the form.
const rows = computed(() =>
  [...props.profiles].reverse().map((profile) => ({
    profile,
    kinds: [
      ...(profile.domains.length ? [t.profiles.kindSite] : []),
      ...(profile.processes.length ? [t.profiles.kindApp] : []),
    ],
    coverage: coverageLines(stillProxied(profile, props.all, isMac.value)),
  })),
);

async function toggle(profile: Profile, enabled: boolean) {
  toggling.value = profile.id;
  await actions.setProfileEnabled(profile.id, enabled);
  toggling.value = null;
}

function confirmRemove(profile: Profile) {
  dialog.warning({
    title: t.profiles.deleteTitle,
    content: t.profiles.deleteBody(profile.name),
    positiveText: t.common.delete,
    negativeText: t.common.cancel,
    onPositiveClick: async () => {
      if (await actions.removeProfile(profile.id)) message.success(t.profiles.deleted);
    },
  });
}
</script>

<template>
  <ul class="rows" :aria-label="t.profiles.customList" :style="{ '--pp-divider': themeVars.dividerColor }">
    <li v-for="row in rows" :key="row.profile.id" class="row">
      <div class="main">
        <div class="line">
          <NTag v-for="kind in row.kinds" :key="kind" size="small" :bordered="false">{{ kind }}</NTag>
          <span class="name">{{ row.profile.name }}</span>
        </div>
        <NText v-for="line in row.coverage" :key="line" type="info" tag="div" class="note">{{ line }}</NText>
      </div>
      <NSwitch
        :value="row.profile.enabled"
        :loading="toggling === row.profile.id"
        :aria-label="t.profiles.switchAria(row.profile.name)"
        @update:value="(enabled: boolean) => toggle(row.profile, enabled)"
      />
      <NButton
        size="small"
        quaternary
        type="error"
        :aria-label="t.profiles.deleteAria(row.profile.name)"
        @click="confirmRemove(row.profile)"
      >
        {{ t.common.delete }}
      </NButton>
    </li>
  </ul>
</template>

<style scoped>
.rows {
  list-style: none;
  margin: 0;
  padding: 0;
}

.row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 0;
  border-top: 1px solid var(--pp-divider);
}

.row:first-child {
  border-top: none;
}

.main {
  flex: 1;
  min-width: 0;
}

.line {
  display: flex;
  align-items: center;
  gap: 8px;
}

.name {
  word-break: break-all;
}

.note {
  margin-top: 2px;
  font-size: 13px;
}
</style>
