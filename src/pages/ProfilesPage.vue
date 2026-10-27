<script setup lang="ts">
import { computed } from "vue";
import { NFlex, NText } from "naive-ui";
import type { AppState } from "../api";
import { t } from "../i18n";
import SectionCard from "../components/SectionCard.vue";
import ProfileCard from "../components/ProfileCard.vue";
import AddSiteForm from "../components/AddSiteForm.vue";
import AddAppForm from "../components/AddAppForm.vue";

const props = defineProps<{ state: AppState }>();

const presets = computed(() => props.state.profiles.filter((p) => p.source === "preset"));
const custom = computed(() => props.state.profiles.filter((p) => p.source === "custom"));
</script>

<template>
  <h1 class="pp-page-title">{{ t.profiles.title }}</h1>
  <NText tag="p" class="lead">{{ t.profiles.intro }}</NText>
  <NText tag="p" depth="3" class="lead">{{ t.profiles.restartNote }}</NText>

  <NFlex vertical :size="16" class="page">
    <SectionCard :title="t.profiles.presets">
      <div class="grid">
        <ProfileCard v-for="profile in presets" :key="profile.id" :profile="profile" />
      </div>
    </SectionCard>

    <SectionCard :title="t.profiles.custom">
      <div v-if="custom.length" class="grid">
        <ProfileCard v-for="profile in custom" :key="profile.id" :profile="profile" />
      </div>
      <NText v-else depth="3">{{ t.profiles.customEmpty }}</NText>
    </SectionCard>

    <AddSiteForm />
    <AddAppForm />
  </NFlex>
</template>

<style scoped>
.lead {
  margin: 0 0 4px;
}

.page {
  margin-top: 12px;
}

.grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
  gap: 12px;
}
</style>
