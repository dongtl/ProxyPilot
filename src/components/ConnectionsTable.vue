<script setup lang="ts">
import { computed, ref } from "vue";
import { NButton, NFlex, NTable, NTag, useMessage } from "naive-ui";
import type { ConnectionRow } from "../api";
import { t } from "../i18n";
import { actions } from "../store";
import { formatDateTime, formatEndpoint, formatRelativeTime, isIpAddress } from "../format";

const props = defineProps<{ rows: ConnectionRow[]; now: number }>();

const message = useMessage();
const pending = ref<string | null>(null);

const sorted = computed(() => [...props.rows].sort((a, b) => b.lastSeen - a.lastSeen));

function rowKey(row: ConnectionRow): string {
  return `${row.process}|${row.host}|${row.port}|${row.route}`;
}

async function proxySite(row: ConnectionRow) {
  pending.value = `site:${rowKey(row)}`;
  if (await actions.addSite(row.host)) message.success(t.connections.addedSite);
  pending.value = null;
}

async function proxyApp(row: ConnectionRow) {
  pending.value = `app:${rowKey(row)}`;
  if (await actions.addApp(row.process)) message.success(t.connections.addedApp);
  pending.value = null;
}
</script>

<template>
  <div class="scroll">
    <NTable size="small" :single-line="false">
      <thead>
        <tr>
          <th scope="col">{{ t.connections.colProcess }}</th>
          <th scope="col">{{ t.connections.colHost }}</th>
          <th scope="col">{{ t.connections.colRoute }}</th>
          <th scope="col" class="num">{{ t.connections.colCount }}</th>
          <th scope="col">{{ t.connections.colLastSeen }}</th>
          <th scope="col">{{ t.connections.colActions }}</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="row in sorted" :key="rowKey(row)">
          <td class="wrap">{{ row.process || t.connections.unknownProcess }}</td>
          <td class="wrap">{{ formatEndpoint(row.host, row.port) }}</td>
          <td>
            <NTag size="small" :type="row.route === 'proxy' ? 'success' : 'default'" :bordered="false">
              {{ row.route === "proxy" ? t.connections.routeProxy : t.connections.routeDirect }}
            </NTag>
          </td>
          <td class="num">{{ row.count }}</td>
          <td class="nowrap" :title="formatDateTime(row.lastSeen)">{{ formatRelativeTime(row.lastSeen, now) }}</td>
          <td>
            <NFlex v-if="row.route === 'direct'" vertical :size="2" align="start">
              <NButton
                v-if="!isIpAddress(row.host)"
                text
                type="primary"
                size="small"
                :loading="pending === `site:${rowKey(row)}`"
                :aria-label="t.connections.actionAria(t.connections.siteToProxy, row.host)"
                @click="proxySite(row)"
              >
                {{ t.connections.siteToProxy }}
              </NButton>
              <NButton
                v-if="row.process"
                text
                type="primary"
                size="small"
                :loading="pending === `app:${rowKey(row)}`"
                :aria-label="t.connections.actionAria(t.connections.appToProxy, row.process)"
                @click="proxyApp(row)"
              >
                {{ t.connections.appToProxy }}
              </NButton>
            </NFlex>
          </td>
        </tr>
      </tbody>
    </NTable>
  </div>
</template>

<style scoped>
.scroll {
  overflow-x: auto;
}

.num {
  text-align: right;
}

.wrap {
  word-break: break-all;
}

.nowrap {
  white-space: nowrap;
}
</style>
