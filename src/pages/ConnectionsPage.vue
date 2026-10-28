<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { NAlert, NEmpty, NFlex, NInput, NRadioButton, NRadioGroup, NSpin, NText } from "naive-ui";
import { api, type AppState, type ConnectionRow } from "../api";
import { t } from "../i18n";
import { errorText } from "../store";
import ConnectionsTable from "../components/ConnectionsTable.vue";

defineProps<{ state: AppState }>();

const POLL_MS = 2000;

const rows = ref<ConnectionRow[]>([]);
const loaded = ref(false);
const error = ref<string | null>(null);
const now = ref(Date.now());
const filter = ref<"all" | "proxy" | "direct">("all");
const search = ref("");

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

// Errors stay inline instead of toasting every two seconds.
async function poll() {
  if (!document.hidden) {
    try {
      rows.value = await api.getConnections();
      error.value = null;
    } catch (err) {
      error.value = errorText(err);
    }
    loaded.value = true;
    now.value = Date.now();
  }
  if (active) timer = setTimeout(poll, POLL_MS);
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
  <h1 class="pp-page-title">{{ t.connections.title }}</h1>
  <NText depth="3">{{ t.connections.intro }}</NText>

  <NFlex vertical :size="16" class="page">
    <NAlert v-if="error" type="warning" :title="t.connections.loadFailed">{{ error }}</NAlert>

    <div v-if="!loaded" class="center" role="status">
      <NSpin size="small" :description="t.connections.loading" />
    </div>
    <NEmpty
      v-else-if="rows.length === 0"
      class="center"
      :description="state.routingEnabled ? t.connections.emptyOn : t.connections.emptyOff"
    />
    <template v-else>
      <NFlex align="center" justify="space-between" :size="12">
        <NRadioGroup v-model:value="filter" size="small" role="radiogroup" :aria-label="t.connections.filterLabel">
          <NRadioButton value="all">{{ t.connections.filterAll(rows.length) }}</NRadioButton>
          <NRadioButton value="proxy">{{ t.connections.filterProxy(proxyCount) }}</NRadioButton>
          <NRadioButton value="direct">{{ t.connections.filterDirect(rows.length - proxyCount) }}</NRadioButton>
        </NRadioGroup>
        <NInput
          v-model:value="search"
          size="small"
          clearable
          class="search"
          :placeholder="t.connections.searchLabel"
          :input-props="{ 'aria-label': t.connections.searchLabel, autocomplete: 'off', spellcheck: false }"
        />
      </NFlex>
      <NEmpty v-if="shown.length === 0" class="center" :description="t.connections.noMatch" />
      <ConnectionsTable v-else :rows="shown" :now="now" />
    </template>
  </NFlex>
</template>

<style scoped>
.page {
  margin-top: 12px;
}

.search {
  width: 220px;
}

.center {
  padding: 40px 0;
}
</style>
