<script setup lang="ts">
// macOS, running outside the Applications folder: ask once when the window
// opens, then keep a slim bar above every page until it is done.
import { computed, ref } from "vue";
import { NAlert, NButton, NFlex, NModal } from "naive-ui";
import type { AppState } from "../api";
import { t } from "../i18n";
import { actions, relocateBusy } from "../store";

const props = defineProps<{ relocation: NonNullable<AppState["relocation"]> }>();

const asked = ref(false);
const moving = computed(() => props.relocation === "move");

async function go() {
  await actions.relocate();
  // Still here means it failed (already toasted); the bar stays for another try.
  asked.value = true;
}
</script>

<template>
  <NModal
    v-if="!asked"
    :show="true"
    preset="dialog"
    type="info"
    :title="moving ? t.relocate.moveTitle : t.relocate.openTitle"
    :content="moving ? t.relocate.moveBody : t.relocate.openBody"
    :positive-text="moving ? t.relocate.moveButton : t.relocate.openButton"
    :negative-text="t.relocate.later"
    :loading="relocateBusy"
    :mask-closable="false"
    :on-positive-click="() => (void go(), false)"
    :on-negative-click="() => (asked = true)"
    @update:show="(show: boolean) => (asked = !show)"
  />
  <NAlert v-else type="warning" :show-icon="true" class="banner">
    <NFlex align="center" justify="space-between" :size="12">
      <span>{{ moving ? t.relocate.bannerMove : t.relocate.bannerOpen }}</span>
      <NButton type="primary" size="small" :loading="relocateBusy" @click="go">
        {{ moving ? t.relocate.bannerMoveButton : t.relocate.bannerOpenButton }}
      </NButton>
    </NFlex>
  </NAlert>
</template>

<style scoped>
.banner {
  margin-bottom: 16px;
}
</style>
