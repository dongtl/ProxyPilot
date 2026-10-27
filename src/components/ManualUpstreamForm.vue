<script setup lang="ts">
import { ref, useId } from "vue";
import { NButton, NForm, NFormItem, NInput, NInputNumber, NRadio, NRadioGroup, NText, useMessage } from "naive-ui";
import type { Upstream, UpstreamKind } from "../api";
import { t } from "../i18n";

defineProps<{ loading: boolean }>();
const emit = defineEmits<{ submit: [upstream: Upstream] }>();

const message = useMessage();
const id = useId();
const kindLabelId = `${id}-kind`;
const hostId = `${id}-host`;
const portId = `${id}-port`;

const kind = ref<UpstreamKind>("socks5");
const host = ref("127.0.0.1");
const port = ref<number | null>(null);
const kinds: UpstreamKind[] = ["socks5", "http"];

function submit() {
  const trimmedHost = host.value.trim();
  if (!trimmedHost) {
    message.warning(t.picker.manualHostEmpty);
    return;
  }
  if (port.value === null || !Number.isInteger(port.value) || port.value < 1 || port.value > 65535) {
    message.warning(t.picker.manualPortInvalid);
    return;
  }
  emit("submit", { kind: kind.value, host: trimmedHost, port: port.value });
}
</script>

<template>
  <NForm label-placement="left" label-width="auto" :show-feedback="false" @submit.prevent="submit">
    <NFormItem :label="t.picker.manualKind" :label-props="{ id: kindLabelId }">
      <NRadioGroup v-model:value="kind" :name="kindLabelId" role="radiogroup" :aria-labelledby="kindLabelId">
        <NRadio v-for="k in kinds" :key="k" :value="k">{{ t.upstream.kind[k] }}</NRadio>
      </NRadioGroup>
    </NFormItem>
    <NFormItem :label="t.picker.manualHost" :label-props="{ for: hostId }" class="row">
      <NInput v-model:value="host" :input-props="{ id: hostId, autocomplete: 'off' }" class="host" />
    </NFormItem>
    <NFormItem :label="t.picker.manualPort" :label-props="{ for: portId }" class="row">
      <NInputNumber
        v-model:value="port"
        :min="1"
        :max="65535"
        :precision="0"
        :show-button="false"
        :placeholder="t.picker.manualPortPlaceholder"
        :input-props="{ id: portId, inputmode: 'numeric' }"
        class="port"
      />
    </NFormItem>
    <NText v-if="kind === 'http'" depth="3" tag="p" class="pp-hint">{{ t.picker.httpHint }}</NText>
    <NButton attr-type="submit" type="primary" :loading="loading" class="submit">
      {{ t.picker.manualSubmit }}
    </NButton>
  </NForm>
</template>

<style scoped>
.row {
  margin-top: 8px;
}

.host {
  max-width: 240px;
}

.port {
  width: 160px;
}

.submit {
  margin-top: 12px;
}
</style>
