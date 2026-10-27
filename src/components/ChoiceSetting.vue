<script setup lang="ts" generic="T extends string">
import { ref, useId } from "vue";
import { NFlex, NRadio, NRadioGroup, NText, useMessage } from "naive-ui";
import { t } from "../i18n";
import SectionCard from "./SectionCard.vue";

const props = defineProps<{
  title: string;
  options: { value: T; label: string; hint: string }[];
  value: T;
  save: (value: T) => Promise<boolean>;
}>();

const message = useMessage();
const headingId = useId();
const pending = ref(false);

async function onUpdate(next: string | number | boolean) {
  pending.value = true;
  const ok = await props.save(next as T);
  pending.value = false;
  if (ok) message.success(t.settings.saved);
}
</script>

<template>
  <SectionCard :title="title" :heading-id="headingId">
    <NRadioGroup
      :value="value"
      :disabled="pending"
      :name="headingId"
      role="radiogroup"
      :aria-labelledby="headingId"
      @update:value="onUpdate"
    >
      <NFlex vertical :size="10">
        <NRadio v-for="option in options" :key="option.value" :value="option.value">
          <span class="label">{{ option.label }}</span>
          <NText depth="3" class="hint">{{ option.hint }}</NText>
        </NRadio>
      </NFlex>
    </NRadioGroup>
    <slot />
  </SectionCard>
</template>

<style scoped>
.label {
  display: block;
}

.hint {
  display: block;
  font-size: 12px;
}
</style>
