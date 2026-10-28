<script setup lang="ts">
import { computed, ref } from "vue";
import { NButton, NDropdown, NTable, NTag, useMessage } from "naive-ui";
import type { ConnectionRow } from "../api";
import { t } from "../i18n";
import { actions } from "../store";
import { formatDateTime, formatRelativeTime, formatTarget, isIpAddress } from "../format";

const props = defineProps<{ rows: ConnectionRow[]; now: number }>();

const message = useMessage();
const pending = ref<string | null>(null);

type Choice = "host" | "site" | "app";

function rowKey(row: ConnectionRow): string {
  return `${row.process}|${row.host}|${row.port}|${row.route}`;
}

/** What "this website" adds: the backend drops a leading "www.", so say so up front. */
function hostDomain(host: string): string {
  return host.replace(/^www\.(?=.+\.)/i, "");
}

/** The ways a direct row can be sent through the proxy instead. */
function choices(row: ConnectionRow): { key: Choice; label: string }[] {
  if (row.route !== "direct") return [];
  const out: { key: Choice; label: string }[] = [];
  if (!isIpAddress(row.host)) out.push({ key: "host", label: t.connections.toProxyHost(hostDomain(row.host)) });
  if (row.site) out.push({ key: "site", label: t.connections.toProxySite(row.site) });
  if (row.process) out.push({ key: "app", label: t.connections.toProxyApp(row.process) });
  return out;
}

const items = computed(() =>
  [...props.rows]
    .sort((a, b) => b.lastSeen - a.lastSeen)
    .map((row) => ({ row, key: rowKey(row), choices: choices(row) })),
);

async function choose(row: ConnectionRow, key: string, choice: Choice) {
  pending.value = key;
  const target = choice === "app" ? row.process : choice === "site" ? row.site! : hostDomain(row.host);
  const ok = choice === "app" ? await actions.addApp(row.process) : await actions.addSite(target);
  if (ok) message.success(t.connections.added(target));
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
        <tr v-for="item in items" :key="item.key">
          <td class="wrap">{{ item.row.process || t.connections.unknownProcess }}</td>
          <td class="wrap">{{ formatTarget(item.row.host, item.row.port) }}</td>
          <td>
            <NTag size="small" :type="item.row.route === 'proxy' ? 'success' : 'default'" :bordered="false">
              {{ item.row.route === "proxy" ? t.connections.routeProxy : t.connections.routeDirect }}
            </NTag>
          </td>
          <td class="num">{{ item.row.count }}</td>
          <td class="nowrap" :title="formatDateTime(item.row.lastSeen)">
            {{ formatRelativeTime(item.row.lastSeen, now) }}
          </td>
          <td class="nowrap">
            <NDropdown
              v-if="item.choices.length"
              trigger="click"
              :options="item.choices"
              @select="(choice: Choice) => choose(item.row, item.key, choice)"
            >
              <NButton
                size="tiny"
                secondary
                type="primary"
                :loading="pending === item.key"
                :aria-label="t.connections.toProxyAria(formatTarget(item.row.host, item.row.port))"
                aria-haspopup="menu"
              >
                {{ t.connections.toProxy }}
              </NButton>
            </NDropdown>
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
