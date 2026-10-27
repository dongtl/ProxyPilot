<script setup lang="ts">
import { computed, h, onMounted, ref, useId } from "vue";
import { NButton, NFlex, NSelect, NText, useMessage, type SelectOption } from "naive-ui";
import { open } from "@tauri-apps/plugin-dialog";
import type { RunningProcess } from "../api";
import { t } from "../i18n";
import { actions, errorText } from "../store";
import { basename } from "../format";
import SectionCard from "./SectionCard.vue";

const message = useMessage();
const selectId = useId();
const processes = ref<RunningProcess[]>([]);
const loadingList = ref(false);
const selected = ref<string | null>(null);
const adding = ref(false);

// The same program often runs several times; one entry per name is enough.
const options = computed((): SelectOption[] => {
  const byName = new Map<string, RunningProcess>();
  for (const p of processes.value) {
    const key = p.name.toLowerCase();
    const seen = byName.get(key);
    if (!seen || (!seen.path && p.path)) byName.set(key, p);
  }
  return [...byName.values()]
    .sort((a, b) => a.name.localeCompare(b.name))
    .map((p) => ({ label: p.name, value: p.name, path: p.path ?? "" }));
});

function optionPath(option: SelectOption): string {
  return typeof option.path === "string" ? option.path : "";
}

// Menu rows show the path so same-named programs can be told apart.
function renderLabel(option: SelectOption) {
  const path = optionPath(option);
  return [String(option.label ?? ""), path ? h("span", { class: "pp-option-path" }, path) : null];
}

// The closed select shows just the name.
function renderTag({ option }: { option: SelectOption }) {
  return String(option.label ?? "");
}

function filter(pattern: string, option: SelectOption): boolean {
  const q = pattern.toLowerCase();
  return String(option.label ?? "").toLowerCase().includes(q) || optionPath(option).toLowerCase().includes(q);
}

async function loadProcesses() {
  loadingList.value = true;
  processes.value = (await actions.listProcesses()) ?? processes.value;
  loadingList.value = false;
}

async function add(process: string) {
  adding.value = true;
  if (await actions.addApp(process)) {
    message.success(t.addApp.added);
    selected.value = null;
  }
  adding.value = false;
}

function addSelected() {
  if (!selected.value) {
    message.warning(t.addApp.noSelection);
    return;
  }
  void add(selected.value);
}

async function browse() {
  try {
    const path = await open({
      title: t.addApp.browseTitle,
      multiple: false,
      directory: false,
      filters: [{ name: t.addApp.exeFilter, extensions: ["exe"] }],
    });
    if (path) await add(basename(path));
  } catch (err) {
    message.error(errorText(err));
  }
}

onMounted(loadProcesses);
</script>

<template>
  <SectionCard :title="t.addApp.title">
    <form @submit.prevent="addSelected">
      <label :for="selectId" class="label">{{ t.addApp.label }}</label>
      <NFlex :wrap="false" :size="8">
        <NSelect
          v-model:value="selected"
          filterable
          clearable
          :options="options"
          :filter="filter"
          :loading="loadingList"
          :placeholder="t.addApp.placeholder"
          :render-label="renderLabel"
          :render-tag="renderTag"
          :input-props="{ id: selectId, 'aria-label': t.addApp.label }"
        />
        <NButton attr-type="submit" type="primary" :loading="adding">{{ t.addApp.submit }}</NButton>
      </NFlex>
      <NFlex :size="8" class="more">
        <NButton size="small" :loading="loadingList" @click="loadProcesses">{{ t.addApp.refresh }}</NButton>
        <NButton size="small" @click="browse">{{ t.addApp.browse }}</NButton>
      </NFlex>
      <NText depth="3" tag="p" class="pp-hint">{{ t.addApp.hint }}</NText>
    </form>
  </SectionCard>
</template>

<style scoped>
.label {
  display: block;
  margin-bottom: 6px;
}

.more {
  margin-top: 8px;
}
</style>
