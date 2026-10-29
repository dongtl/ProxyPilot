<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { NAlert, NButton, NEmpty, NFlex, NInput, NRadioButton, NRadioGroup, NSpin, NText, useMessage } from "naive-ui";
import { api, type AppState, type ConnectionRow } from "../api";
import { t } from "../i18n";
import { errorText } from "../store";
import ConnectionsTable from "../components/ConnectionsTable.vue";

defineProps<{ state: AppState }>();

const POLL_MS = 2000;

const message = useMessage();
const rows = ref<ConnectionRow[]>([]);
const loaded = ref(false);
const error = ref<string | null>(null);
const now = ref(Date.now());
const filter = ref<"all" | "proxy" | "direct">("all");
const search = ref("");
// The list only changes while neither is set: paused by the user, or a row's menu is open.
const paused = ref(false);
const menuOpen = ref(false);
const clearing = ref(false);

const proxyCount = computed(() => rows.value.filter((row) => row.route === "proxy").length);

const shown = computed(() => {
  const query = search.value.trim().toLowerCase();
  return rows.value.filter(
    (row) =>
      (filter.value === "all" || row.route === filter.value) &&
      (!query || row.process.toLowerCase().includes(query) || row.host.toLowerCase().includes(query)),
  );
});

let timer: ReturnType<typeof setTimeout> | null = null;
let active = false;
// Bumped by "清空", so a poll already on its way cannot bring the old rows back.
let generation = 0;

// Errors stay inline instead of toasting every two seconds.
async function poll() {
  if (!document.hidden && !paused.value && !menuOpen.value) {
    const started = generation;
    try {
      const next = await api.getConnections();
      if (started === generation) rows.value = next;
      error.value = null;
    } catch (err) {
      error.value = errorText(err);
    }
    loaded.value = true;
    now.value = Date.now();
  }
  if (active) timer = setTimeout(poll, POLL_MS);
}

async function clear() {
  clearing.value = true;
  try {
    await api.clearConnections();
    generation += 1;
    rows.value = [];
    message.success(t.connections.cleared);
  } catch (err) {
    message.error(errorText(err));
  }
  clearing.value = false;
}

onMounted(() => {
  active = true;
  void poll();
});

onBeforeUnmount(() => {
  active = false;
  if (timer) clearTimeout(timer);
});
</script>

<template>
  <div class="page">
    <h1 class="pp-page-title">{{ t.connections.title }}</h1>
    <NText depth="3">{{ t.connections.intro }}</NText>

    <NAlert v-if="error" type="warning" :title="t.connections.loadFailed" class="block">{{ error }}</NAlert>

    <div v-if="!loaded" class="center" role="status">
      <NSpin size="small" :description="t.connections.loading" />
    </div>
    <template v-else>
      <NFlex align="center" justify="space-between" :size="12" class="block">
        <NRadioGroup v-model:value="filter" size="small" role="radiogroup" :aria-label="t.connections.filterLabel">
          <NRadioButton value="all">{{ t.connections.filterAll(rows.length) }}</NRadioButton>
          <NRadioButton value="proxy">{{ t.connections.filterProxy(proxyCount) }}</NRadioButton>
          <NRadioButton value="direct">{{ t.connections.filterDirect(rows.length - proxyCount) }}</NRadioButton>
        </NRadioGroup>
        <NFlex align="center" :size="8">
          <NText v-if="paused" type="warning" class="paused" role="status">{{ t.connections.paused }}</NText>
          <NInput
            v-model:value="search"
            size="small"
            clearable
            class="search"
            :placeholder="t.connections.searchLabel"
            :input-props="{ 'aria-label': t.connections.searchLabel, autocomplete: 'off', spellcheck: false }"
          />
          <NButton size="small" :type="paused ? 'primary' : 'default'" :aria-pressed="paused" @click="paused = !paused">
            {{ paused ? t.connections.resume : t.connections.pause }}
          </NButton>
          <NButton size="small" :loading="clearing" :disabled="rows.length === 0" @click="clear">
            {{ t.connections.clear }}
          </NButton>
        </NFlex>
      </NFlex>

      <NEmpty
        v-if="rows.length === 0"
        class="center"
        :description="state.routingEnabled ? t.connections.emptyOn : t.connections.emptyOff"
      />
      <NEmpty v-else-if="shown.length === 0" class="center" :description="t.connections.noMatch" />
      <!-- The table fills the rest of the window and scrolls on its own; the controls above stay put. -->
      <div v-else class="table-area">
        <div class="table-fill">
          <ConnectionsTable :rows="shown" :now="now" @menu="(open) => (menuOpen = open)" />
        </div>
      </div>
    </template>
  </div>
</template>

<style scoped>
.page {
  display: flex;
  flex-direction: column;
  flex: 1 0 auto;
}

.block {
  margin-top: 12px;
}

.search {
  width: 200px;
}

.paused {
  font-size: 13px;
  white-space: nowrap;
}

.center {
  padding: 40px 0;
}

/* A definite height for the table to fill; below 200px the page scrolls instead. */
.table-area {
  position: relative;
  flex: 1;
  min-height: 200px;
  margin-top: 12px;
}

.table-fill {
  position: absolute;
  inset: 0;
}
</style>
