<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { NButton, NResult, NSpin, NText, useMessage, useThemeVars } from "naive-ui";
import { t } from "../i18n";
import { appState, disposeStore, initStore, loadError, setErrorNotifier } from "../store";
import { currentPage, goTo, type PageKey } from "../navigation";
import HomePage from "../pages/HomePage.vue";
import ProfilesPage from "../pages/ProfilesPage.vue";
import ConnectionsPage from "../pages/ConnectionsPage.vue";
import SettingsPage from "../pages/SettingsPage.vue";

const message = useMessage();
const themeVars = useThemeVars();
setErrorNotifier((text) => message.error(text, { duration: 6000, closable: true }));

const pages: { key: PageKey; label: string }[] = [
  { key: "home", label: t.nav.home },
  { key: "profiles", label: t.nav.profiles },
  { key: "connections", label: t.nav.connections },
  { key: "settings", label: t.nav.settings },
];

const mainEl = ref<HTMLElement | null>(null);

// Move focus to the new page so keyboard and screen-reader users start at its top.
watch(currentPage, async () => {
  await nextTick();
  if (!mainEl.value) return;
  mainEl.value.scrollTop = 0;
  mainEl.value.focus();
});

onMounted(initStore);
onBeforeUnmount(disposeStore);
</script>

<template>
  <div class="pp-app shell" :style="{ '--pp-focus': themeVars.primaryColor }">
    <nav class="nav" :aria-label="t.nav.label" :style="{ borderRightColor: themeVars.dividerColor }">
      <div class="brand">
        <div class="brand-name">{{ t.appName }}</div>
        <NText depth="3" class="brand-sub">{{ t.appSubtitle }}</NText>
      </div>
      <NButton
        v-for="page in pages"
        :key="page.key"
        class="nav-item"
        block
        :quaternary="currentPage !== page.key"
        :secondary="currentPage === page.key"
        :type="currentPage === page.key ? 'primary' : 'default'"
        :aria-current="currentPage === page.key ? 'page' : undefined"
        @click="goTo(page.key)"
      >
        {{ page.label }}
      </NButton>
    </nav>

    <main ref="mainEl" class="main" tabindex="-1">
      <div v-if="appState" class="content">
        <HomePage v-if="currentPage === 'home'" :state="appState" />
        <ProfilesPage v-else-if="currentPage === 'profiles'" :state="appState" />
        <ConnectionsPage v-else-if="currentPage === 'connections'" :state="appState" />
        <SettingsPage v-else :state="appState" />
      </div>
      <NResult
        v-else-if="loadError"
        class="boot-error"
        status="error"
        :title="t.boot.failedTitle"
        :description="loadError"
      >
        <template #footer>
          <NButton type="primary" @click="initStore">{{ t.boot.retry }}</NButton>
        </template>
      </NResult>
      <div v-else class="boot" role="status">
        <NSpin :description="t.boot.loading" />
      </div>
    </main>
  </div>
</template>

<style scoped>
.shell {
  display: flex;
  height: 100%;
}

.nav {
  width: 150px;
  flex-shrink: 0;
  box-sizing: border-box;
  padding: 16px 12px;
  border-right: 1px solid;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.brand {
  padding: 0 8px 16px;
}

.brand-name {
  font-size: 18px;
  font-weight: 600;
}

.brand-sub {
  font-size: 12px;
}

.nav-item {
  justify-content: flex-start;
}

.main {
  flex: 1;
  overflow-y: auto;
  outline: none;
}

.content {
  box-sizing: border-box;
  max-width: 880px;
  margin: 0 auto;
  padding: 20px 24px 32px;
}

.boot {
  display: flex;
  justify-content: center;
  padding-top: 30vh;
}

.boot-error {
  padding-top: 15vh;
}
</style>
