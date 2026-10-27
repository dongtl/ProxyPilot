<script setup lang="ts">
import { NFlex, NText, useMessage } from "naive-ui";
import type { AppState } from "../api";
import { t } from "../i18n";
import SectionCard from "../components/SectionCard.vue";
import RoutingCard from "../components/RoutingCard.vue";
import UpstreamCard from "../components/UpstreamCard.vue";
import UpstreamPicker from "../components/UpstreamPicker.vue";
import EnvFindings from "../components/EnvFindings.vue";
import ProfileSummary from "../components/ProfileSummary.vue";
import DiagnosticsCard from "../components/DiagnosticsCard.vue";

defineProps<{ state: AppState }>();

const message = useMessage();

function onPicked() {
  message.success(t.home.picked, { duration: 6000 });
}
</script>

<template>
  <h1 class="pp-page-title">{{ t.nav.home }}</h1>
  <NFlex vertical :size="16" class="page">
    <template v-if="state.upstream">
      <RoutingCard :state="state" />
      <UpstreamCard :state="state" :upstream="state.upstream" />
    </template>
    <SectionCard v-else :title="t.home.firstRunTitle">
      <NText tag="p" class="intro">{{ t.picker.intro }}</NText>
      <UpstreamPicker auto-detect @picked="onPicked" />
    </SectionCard>

    <EnvFindings :findings="state.env" />
    <ProfileSummary v-if="state.upstream" :profiles="state.profiles" />
    <DiagnosticsCard v-if="state.upstream" :state="state" />
  </NFlex>
</template>

<style scoped>
.page {
  margin-top: 12px;
}

.intro {
  margin: 0 0 12px;
}
</style>
