<script setup lang="ts">
import { onMounted, ref, useId } from "vue";
import { NButton, NFlex, NInput, NText, useMessage, type InputInst } from "naive-ui";
import { t } from "../i18n";
import { actions } from "../store";

const message = useMessage();
const inputId = useId();
const input = ref<InputInst | null>(null);
const value = ref("");
const pending = ref(false);

// Opened by a button, so the user wants to type right away.
onMounted(() => input.value?.focus());

// The form stays open after adding, so several sites can go in one after another.
async function submit() {
  const input = value.value.trim();
  if (!input) {
    message.warning(t.common.emptyInput);
    return;
  }
  pending.value = true;
  if (await actions.addSite(input)) {
    message.success(t.addSite.added);
    value.value = "";
  }
  pending.value = false;
}
</script>

<template>
  <form @submit.prevent="submit">
    <label :for="inputId" class="label">{{ t.addSite.label }}</label>
    <NFlex :wrap="false" :size="8">
      <NInput
        ref="input"
        v-model:value="value"
        clearable
        :placeholder="t.addSite.placeholder"
        :input-props="{ id: inputId, autocomplete: 'off', spellcheck: false }"
      />
      <NButton attr-type="submit" type="primary" :loading="pending">{{ t.addSite.submit }}</NButton>
    </NFlex>
    <NText depth="3" tag="p" class="pp-hint">{{ t.addSite.hint }}</NText>
  </form>
</template>

<style scoped>
.label {
  display: block;
  margin-bottom: 6px;
}
</style>
