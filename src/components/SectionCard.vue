<script setup lang="ts">
// NCard renders its title as a plain div; this keeps a real heading for
// screen-reader navigation and lets inputs reference it via aria-labelledby.
import { NCard } from "naive-ui";

defineProps<{
  title: string;
  headingId?: string;
}>();
</script>

<template>
  <NCard tag="section" :aria-labelledby="headingId">
    <template #header>
      <h2 :id="headingId" class="pp-section-title">{{ title }}</h2>
    </template>
    <template v-if="$slots.extra" #header-extra>
      <slot name="extra" />
    </template>
    <slot />
  </NCard>
</template>
