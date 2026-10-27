<script setup lang="ts">
import { ref, useId } from "vue";
import { NButton, NFlex, NInput, NText, useMessage } from "naive-ui";
import { t } from "../i18n";
import SectionCard from "./SectionCard.vue";

const props = defineProps<{
  title: string;
  hint: string;
  label: string;
  placeholder: string;
  emptyText: string;
  items: string[];
  normalize: (input: string) => string;
  /** Persists the whole new list; resolves true on success. */
  save: (items: string[]) => Promise<boolean>;
}>();

const message = useMessage();
const inputId = useId();
const draft = ref("");
const pending = ref(false);

async function commit(next: string[]): Promise<boolean> {
  pending.value = true;
  const ok = await props.save(next);
  pending.value = false;
  if (ok) message.success(t.settings.saved);
  return ok;
}

async function add() {
  const value = props.normalize(draft.value);
  if (!value) {
    message.warning(t.common.emptyInput);
    return;
  }
  if (props.items.some((item) => item.toLowerCase() === value.toLowerCase())) {
    message.warning(t.settings.duplicate);
    return;
  }
  if (await commit([...props.items, value])) draft.value = "";
}

function remove(item: string) {
  void commit(props.items.filter((i) => i !== item));
}
</script>

<template>
  <SectionCard :title="title">
    <NText depth="3" tag="p" class="hint">{{ hint }}</NText>

    <ul v-if="items.length" class="items">
      <li v-for="item in items" :key="item" class="item">
        <span class="name">{{ item }}</span>
        <NButton size="tiny" quaternary :disabled="pending" :aria-label="t.settings.removeAria(item)" @click="remove(item)">
          {{ t.common.remove }}
        </NButton>
      </li>
    </ul>
    <NText v-else depth="3" tag="p" class="hint">{{ emptyText }}</NText>

    <form class="add" @submit.prevent="add">
      <label :for="inputId" class="label">{{ label }}</label>
      <NFlex :wrap="false" :size="8">
        <NInput
          v-model:value="draft"
          :placeholder="placeholder"
          :input-props="{ id: inputId, autocomplete: 'off', spellcheck: false }"
        />
        <NButton attr-type="submit" :loading="pending">{{ t.common.add }}</NButton>
      </NFlex>
    </form>
  </SectionCard>
</template>

<style scoped>
.hint {
  margin: 0 0 8px;
}

.items {
  list-style: none;
  margin: 0 0 8px;
  padding: 0;
  display: flex;
  flex-wrap: wrap;
  gap: 6px 16px;
}

.item {
  display: flex;
  align-items: center;
  gap: 4px;
}

.name {
  font-family: ui-monospace, Consolas, monospace;
  font-size: 13px;
}

.add {
  margin-top: 4px;
}

.label {
  display: block;
  margin-bottom: 6px;
}
</style>
