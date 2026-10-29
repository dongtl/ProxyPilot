<script setup lang="ts">
import { computed, ref } from "vue";
import { NButton, NFlex, NText, useThemeVars } from "naive-ui";
import type { AppState } from "../api";
import { t } from "../i18n";
import SectionCard from "../components/SectionCard.vue";
import ProfileCard from "../components/ProfileCard.vue";
import CustomProfileList from "../components/CustomProfileList.vue";
import AddSiteForm from "../components/AddSiteForm.vue";
import AddAppForm from "../components/AddAppForm.vue";

const props = defineProps<{ state: AppState }>();

const themeVars = useThemeVars();
const presets = computed(() => props.state.profiles.filter((p) => p.source === "preset"));
const custom = computed(() => props.state.profiles.filter((p) => p.source === "custom"));

// The add forms open right above the list they add to. Mounting the app form
// only when asked also spares listing the running programs on every visit.
const adding = ref<"site" | "app" | null>(null);

function open(form: "site" | "app") {
  adding.value = adding.value === form ? null : form;
}
</script>

<template>
  <h1 class="pp-page-title">{{ t.profiles.title }}</h1>
  <NText tag="p" class="lead">{{ t.profiles.intro }}</NText>
  <NText tag="p" depth="3" class="lead">{{ t.profiles.restartNote }}</NText>

  <NFlex vertical :size="16" class="page">
    <SectionCard :title="t.profiles.presets">
      <div class="grid">
        <ProfileCard v-for="profile in presets" :key="profile.id" :profile="profile" :all="state.profiles" />
      </div>
    </SectionCard>

    <SectionCard :title="t.profiles.custom">
      <template #extra>
        <NFlex :size="8">
          <NButton
            size="small"
            :type="adding === 'site' ? 'primary' : 'default'"
            :aria-expanded="adding === 'site'"
            @click="open('site')"
          >
            {{ adding === "site" ? t.profiles.closeForm : t.profiles.addSite }}
          </NButton>
          <NButton
            size="small"
            :type="adding === 'app' ? 'primary' : 'default'"
            :aria-expanded="adding === 'app'"
            @click="open('app')"
          >
            {{ adding === "app" ? t.profiles.closeForm : t.profiles.addApp }}
          </NButton>
        </NFlex>
      </template>

      <div v-if="adding" class="form" :style="{ background: themeVars.actionColor }">
        <AddSiteForm v-if="adding === 'site'" />
        <AddAppForm v-else />
      </div>
      <CustomProfileList v-if="custom.length" :profiles="custom" :all="state.profiles" />
      <NText v-else depth="3">{{ t.profiles.customEmpty }}</NText>
    </SectionCard>
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

.form {
  margin-bottom: 12px;
  padding: 12px;
  border-radius: 6px;
}
</style>
