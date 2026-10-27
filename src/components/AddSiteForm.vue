<script setup lang="ts">
import { ref, useId } from "vue";
import { NButton, NFlex, NInput, NText, useMessage } from "naive-ui";
import { t } from "../i18n";
import { actions } from "../store";
import SectionCard from "./SectionCard.vue";

const message = useMessage();
const inputId = useId();
const value = ref("");
const pending = ref(false);

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
  <SectionCard :title="t.addSite.title">
    <form @submit.prevent="submit">
      <label :for="inputId" class="label">{{ t.addSite.label }}</label>
      <NFlex :wrap="false" :size="8">
        <NInput
          v-model:value="value"
          clearable
          :placeholder="t.addSite.placeholder"
          :input-props="{ id: inputId, autocomplete: 'off', spellcheck: false }"
        />
        <NButton attr-type="submit" type="primary" :loading="pending">{{ t.addSite.submit }}</NButton>
      </NFlex>
      <NText depth="3" tag="p" class="pp-hint">{{ t.addSite.hint }}</NText>
    </form>
  </SectionCard>
</template>

<style scoped>
.label {
  display: block;
  margin-bottom: 6px;
}
</style>
