<script setup lang="ts">
import { computed, h, ref } from "vue";
import { NButton, NDataTable, NDropdown, NTag, useMessage, type DataTableColumns } from "naive-ui";
import type { ConnectionRow } from "../api";
import { t } from "../i18n";
import { actions } from "../store";
import { formatDateTime, formatRelativeTime, formatTarget, isIpAddress } from "../format";

const props = defineProps<{ rows: ConnectionRow[]; now: number }>();
// An open "改走代理" menu: the page holds the list still so the row cannot move away under it.
const emit = defineEmits<{ menu: [open: boolean] }>();

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

async function choose(row: ConnectionRow, choice: Choice) {
  pending.value = rowKey(row);
  const target = choice === "app" ? row.process : choice === "site" ? row.site! : hostDomain(row.host);
  const ok = choice === "app" ? await actions.addApp(row.process) : await actions.addSite(target);
  if (ok) message.success(t.connections.added(target));
  pending.value = null;
}

function renderActions(row: ConnectionRow) {
  const options = choices(row);
  if (!options.length) return null;
  return h(
    NDropdown,
    {
      trigger: "click",
      options,
      onSelect: (choice: Choice) => choose(row, choice),
      onUpdateShow: (open: boolean) => emit("menu", open),
    },
    () =>
      h(
        NButton,
        {
          size: "tiny",
          secondary: true,
          type: "primary",
          loading: pending.value === rowKey(row),
          "aria-label": t.connections.toProxyAria(formatTarget(row.host, row.port)),
          "aria-haspopup": "menu",
        },
        () => t.connections.toProxy,
      ),
  );
}

// Long names end in "…" with the full text on hover, so every row is one line
// high and only the rows on screen are drawn, however long the list gets.
const columns = computed((): DataTableColumns<ConnectionRow> => [
  {
    key: "process",
    title: t.connections.colProcess,
    minWidth: 110,
    ellipsis: { tooltip: true },
    render: (row) => row.process || t.connections.unknownProcess,
  },
  {
    key: "host",
    title: t.connections.colHost,
    minWidth: 160,
    ellipsis: { tooltip: true },
    render: (row) => formatTarget(row.host, row.port),
  },
  {
    key: "route",
    title: t.connections.colRoute,
    width: 64,
    render: (row) =>
      h(
        NTag,
        { size: "small", bordered: false, type: row.route === "proxy" ? "success" : "default" },
        () => (row.route === "proxy" ? t.connections.routeProxy : t.connections.routeDirect),
      ),
  },
  { key: "count", title: t.connections.colCount, width: 60, align: "right" },
  {
    key: "lastSeen",
    title: t.connections.colLastSeen,
    width: 88,
    render: (row) => h("span", { title: formatDateTime(row.lastSeen) }, formatRelativeTime(row.lastSeen, props.now)),
  },
  { key: "actions", title: t.connections.colActions, width: 104, render: renderActions },
]);
</script>

<template>
  <NDataTable
    class="table"
    size="small"
    virtual-scroll
    flex-height
    :min-row-height="40"
    :columns="columns"
    :data="rows"
    :row-key="rowKey"
    :bordered="false"
  />
</template>

<style scoped>
.table {
  height: 100%;
}
</style>
