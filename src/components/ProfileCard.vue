<script setup lang="ts">
import { computed, ref, useId } from "vue";
import { NButton, NCard, NFlex, NSwitch, NText } from "naive-ui";
import type { Profile } from "../api";
import { t } from "../i18n";
import { actions, isMac } from "../store";
import { coverageLines, programsFor, stillProxied } from "../coverage";

// Preset services only; the user's own entries are rows in CustomProfileList.
const props = defineProps<{ profile: Profile; all: Profile[] }>();

const headingId = useId();
const contentsId = useId();
const toggling = ref(false);
// Domain lists mean little to most people; the description says what the preset is for.
const expanded = ref(false);

const programs = computed(() => programsFor(props.profile.processes, isMac.value));
const coverage = computed(() => coverageLines(stillProxied(props.profile, props.all, isMac.value)));
const stateText = computed(() =>
  props.profile.enabled ? t.profiles.enabled : coverage.value.length ? t.profiles.partly : t.profiles.disabled,
);

async function toggle(enabled: boolean) {
  toggling.value = true;
  await actions.setProfileEnabled(props.profile.id, enabled);
  toggling.value = false;
}
</script>

<template>
  <NCard tag="article" size="small" embedded :aria-labelledby="headingId">
    <template #header>
      <h3 :id="headingId" class="pp-item-title">{{ profile.name }}</h3>
    </template>
    <template #header-extra>
      <NFlex align="center" :size="8" :wrap="false">
        <NText depth="3" class="state">{{ stateText }}</NText>
        <NSwitch
          :value="profile.enabled"
          :loading="toggling"
          :aria-label="t.profiles.switchAria(profile.name)"
          @update:value="toggle"
        />
      </NFlex>
    </template>

    <NText v-if="profile.description" tag="p" class="desc">{{ profile.description }}</NText>
    <NText v-for="line in coverage" :key="line" type="info" tag="p" class="note">{{ line }}</NText>
    <div v-show="expanded" :id="contentsId">
      <NText v-if="profile.domains.length" depth="3" tag="div" class="list">
        {{ t.profiles.domains }}{{ profile.domains.join(t.common.listSeparator) }}
      </NText>
      <NText v-if="programs.length" depth="3" tag="div" class="list">
        {{ t.profiles.processes }}{{ programs.join(t.common.listSeparator) }}
      </NText>
    </div>

    <template #action>
      <NButton text size="small" :aria-expanded="expanded" :aria-controls="contentsId" @click="expanded = !expanded">
        {{ expanded ? t.profiles.hideContents : t.profiles.showContents }}
      </NButton>
    </template>
  </NCard>
</template>

<style scoped>
.state {
  font-size: 12px;
  white-space: nowrap;
}

.desc {
  margin: 0 0 6px;
}

.note {
  margin: 0 0 6px;
  font-size: 13px;
}

.list {
  font-size: 13px;
  word-break: break-all;
}
</style>
