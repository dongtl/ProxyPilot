<script setup lang="ts">
import { ref, useId } from "vue";
import { NAlert, NButton, NFlex, NSwitch, NText, useMessage } from "naive-ui";
import type { AppState, DnsMode, LogLevel } from "../api";
import { t } from "../i18n";
import { actions } from "../store";
import { normalizeDomainSuffix, normalizeProcessName } from "../format";
import SectionCard from "../components/SectionCard.vue";
import UpstreamSettings from "../components/UpstreamSettings.vue";
import EditableList from "../components/EditableList.vue";
import ChoiceSetting from "../components/ChoiceSetting.vue";
import AboutCard from "../components/AboutCard.vue";
import UpdateSettings from "../components/UpdateSettings.vue";

defineProps<{ state: AppState }>();

const message = useMessage();
const autostartHeadingId = useId();
const autostartPending = ref(false);

const dnsOptions: { value: DnsMode; label: string; hint: string }[] = [
  { value: "encrypted", label: t.settings.dnsEncrypted, hint: t.settings.dnsEncryptedHint },
  { value: "system", label: t.settings.dnsSystem, hint: t.settings.dnsSystemHint },
];

const logOptions: { value: LogLevel; label: string; hint: string }[] = [
  { value: "warn", label: t.settings.logWarn, hint: t.settings.logWarnHint },
  { value: "info", label: t.settings.logInfo, hint: t.settings.logInfoHint },
  { value: "debug", label: t.settings.logDebug, hint: t.settings.logDebugHint },
];

const saveDirect = (directProcesses: string[]) => actions.updateSettings({ directProcesses });
const saveIntranet = (intranetDomains: string[]) => actions.updateSettings({ intranetDomains });
const saveDns = (dnsMode: DnsMode) => actions.updateSettings({ dnsMode });
const saveLog = (logLevel: LogLevel) => actions.updateSettings({ logLevel });

async function setAutostart(autostart: boolean) {
  autostartPending.value = true;
  if (await actions.updateSettings({ autostart })) message.success(t.settings.saved);
  autostartPending.value = false;
}
</script>

<template>
  <h1 class="pp-page-title">{{ t.settings.title }}</h1>

  <NFlex vertical :size="16" class="page">
    <UpstreamSettings :state="state" />

    <EditableList
      :title="t.settings.directTitle"
      :hint="t.settings.directHint"
      :label="t.settings.directLabel"
      :placeholder="t.settings.directPlaceholder"
      :empty-text="t.settings.directEmpty"
      :items="state.directProcesses"
      :normalize="normalizeProcessName"
      :save="saveDirect"
    />

    <EditableList
      :title="t.settings.intranetTitle"
      :hint="t.settings.intranetHint"
      :label="t.settings.intranetLabel"
      :placeholder="t.settings.intranetPlaceholder"
      :empty-text="t.settings.intranetEmpty"
      :items="state.intranetDomains"
      :normalize="normalizeDomainSuffix"
      :save="saveIntranet"
    />

    <ChoiceSetting :title="t.settings.dnsTitle" :options="dnsOptions" :value="state.dnsMode" :save="saveDns" />

    <ChoiceSetting :title="t.settings.logTitle" :options="logOptions" :value="state.logLevel" :save="saveLog">
      <NAlert v-if="state.logLevel !== 'warn'" type="warning" class="log-warning">
        {{ t.settings.logPrivacy }}
      </NAlert>
    </ChoiceSetting>

    <SectionCard :title="t.settings.autostartTitle" :heading-id="autostartHeadingId">
      <NFlex align="center" justify="space-between" :wrap="false">
        <NText depth="3">{{ t.settings.autostartHint }}</NText>
        <NSwitch
          :value="state.autostart"
          :loading="autostartPending"
          :aria-labelledby="autostartHeadingId"
          @update:value="setAutostart"
        />
      </NFlex>
    </SectionCard>

    <SectionCard :title="t.settings.dataDirTitle">
      <NText depth="3" tag="p" class="hint">{{ t.settings.dataDirHint }}</NText>
      <NFlex align="center" justify="space-between">
        <NText code class="path">{{ state.dataDir }}</NText>
        <NButton @click="actions.openDataDir">{{ t.settings.openDataDir }}</NButton>
      </NFlex>
    </SectionCard>

    <UpdateSettings :state="state" />

    <AboutCard :version="state.version" />
  </NFlex>
</template>

<style scoped>
.page {
  margin-top: 12px;
}

.hint {
  margin: 0 0 8px;
}

.path {
  word-break: break-all;
}

.log-warning {
  margin-top: 12px;
}
</style>
