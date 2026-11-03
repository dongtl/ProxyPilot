<script setup lang="ts">
import { computed, ref } from "vue";
import { NButton, NFlex, NInput, NText, useThemeVars } from "naive-ui";
import type { AppState, PresetCategory, Profile } from "../api";
import { t } from "../i18n";
import SectionCard from "../components/SectionCard.vue";
import ProfileCard from "../components/ProfileCard.vue";
import CustomProfileList from "../components/CustomProfileList.vue";
import AddSiteForm from "../components/AddSiteForm.vue";
import AddAppForm from "../components/AddAppForm.vue";

const props = defineProps<{ state: AppState }>();

const CATEGORIES: PresetCategory[] = ["ai", "web", "social", "media", "dev"];

const themeVars = useThemeVars();
const custom = computed(() => props.state.profiles.filter((p) => p.source === "custom"));
const query = ref("");

/** By name, other names ("推特"), description or site, so "twimg" finds X too. */
function matches(profile: Profile, text: string): boolean {
  const haystack = [profile.name, profile.description, ...profile.keywords, ...profile.domains];
  return haystack.some((item) => item.toLowerCase().includes(text));
}

const groups = computed(() => {
  const text = query.value.trim().toLowerCase();
  const presets = props.state.profiles.filter((p) => p.source === "preset" && (!text || matches(p, text)));
  return CATEGORIES.map((category) => ({
    category,
    title: t.profiles.categories[category],
    profiles: presets.filter((p) => p.category === category),
  })).filter((group) => group.profiles.length);
});

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

  <!-- The user's own list first: with a few dozen services below it, it would otherwise sit far down the page. -->
  <NFlex vertical :size="16" class="page">
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

    <SectionCard :title="t.profiles.presets">
      <template #extra>
        <NInput
          v-model:value="query"
          size="small"
          clearable
          class="search"
          :placeholder="t.profiles.searchPlaceholder"
          :input-props="{ 'aria-label': t.profiles.searchLabel, autocomplete: 'off', spellcheck: false }"
        />
      </template>
      <section v-for="group in groups" :key="group.category" class="group" :aria-label="group.title">
        <h3 class="group-title">{{ group.title }}</h3>
        <div class="grid">
          <ProfileCard v-for="profile in group.profiles" :key="profile.id" :profile="profile" :all="state.profiles" />
        </div>
      </section>
      <NText v-if="!groups.length" depth="3" tag="p" class="empty">{{ t.profiles.searchEmpty(query.trim()) }}</NText>
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

.search {
  width: 220px;
}

.group + .group {
  margin-top: 16px;
}

.group-title {
  margin: 0 0 8px;
  font-size: 14px;
  font-weight: 600;
}

/* Three to a row in the default window: with this many services, two made a very long page. */
.grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 12px;
}

.empty {
  margin: 0;
}

.form {
  margin-bottom: 12px;
  padding: 12px;
  border-radius: 6px;
}
</style>
