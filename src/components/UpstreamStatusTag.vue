<script setup lang="ts">
import { computed } from "vue";
import { NTag } from "naive-ui";
import type { UpstreamStatus } from "../api";
import { t } from "../i18n";

const props = defineProps<{ status: UpstreamStatus }>();

const type = computed(() => {
  if (props.status === "ok") return "success";
  if (props.status === "unreachable") return "error";
  return "default";
});
</script>

<template>
  <NTag :type="type" size="small" :bordered="false">{{ t.upstream.status[status] }}</NTag>
</template>
