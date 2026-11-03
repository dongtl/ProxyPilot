<script setup lang="ts">
// Sites added to an item because its pages needed them, each removable again.
import { ref } from "vue";
import { NButton, NTag, NText, useMessage } from "naive-ui";
import type { Profile } from "../api";
import { t } from "../i18n";
import { actions } from "../store";

const props = defineProps<{ profile: Profile }>();

const message = useMessage();
const pending = ref<string | null>(null);

async function remove(site: string) {
  pending.value = site;
  if (await actions.removeExtraSite(props.profile.id, site)) message.success(t.profiles.extraRemoved);
  pending.value = null;
}
</script>

<template>
  <div v-if="profile.extraDomains.length" class="extras">
    <NText depth="3">{{ t.profiles.extraLabel }}</NText>
    <NTag v-for="site in profile.extraDomains" :key="site" size="small" :bordered="false">
      {{ site }}
      <NButton
        text
        size="tiny"
        class="remove"
        :loading="pending === site"
        :aria-label="t.profiles.extraRemoveAria(site)"
        @click="remove(site)"
      >
        ×
      </NButton>
    </NTag>
  </div>
</template>

<style scoped>
.extras {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 4px;
  margin: 2px 0 6px;
  font-size: 13px;
}

.remove {
  margin-left: 4px;
}
</style>
