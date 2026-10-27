<script setup lang="ts">
import { computed } from "vue";
import { NButton, NFlex, NTag, NText } from "naive-ui";
import type { Profile } from "../api";
import { t } from "../i18n";
import { goTo } from "../navigation";
import SectionCard from "./SectionCard.vue";

const props = defineProps<{ profiles: Profile[] }>();

const enabled = computed(() => props.profiles.filter((p) => p.enabled));
</script>

<template>
  <SectionCard :title="t.home.summaryTitle">
    <template #extra>
      <NButton size="small" quaternary @click="goTo('profiles')">{{ t.home.manage }}</NButton>
    </template>
    <template v-if="enabled.length">
      <NText tag="p" class="lead">{{ t.home.summaryCount(enabled.length) }}</NText>
      <NFlex :size="[8, 8]" role="list">
        <NTag v-for="profile in enabled" :key="profile.id" role="listitem" :bordered="false">
          {{ profile.name }}
        </NTag>
      </NFlex>
    </template>
    <NText v-else depth="3">{{ t.home.summaryNone }}</NText>
  </SectionCard>
</template>

<style scoped>
.lead {
  margin: 0 0 8px;
}
</style>
