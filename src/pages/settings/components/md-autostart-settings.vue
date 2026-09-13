<script setup lang="ts">
import { onMounted, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import MdSwitch from '@/components/custom/md-switch.vue';
import MdIcon from '@/components/icons/md-icon.vue';
import { ICON_NAMES } from '@/lib/models/ui';
import { ResidentService } from '@/lib/services/resident-service';

const { t } = useI18n({ useScope: 'global' });
const autostart = ref<boolean | null>(null);
const autostartBusy = ref(false);
const autostartError = ref(false);

async function loadAutostart() {
  autostartBusy.value = true;
  autostartError.value = false;
  try {
    autostart.value = await ResidentService.autostartEnabled();
  } catch {
    autostartError.value = true;
  } finally {
    autostartBusy.value = false;
  }
}
async function saveAutostart(enabled: boolean) {
  if (autostartBusy.value || autostart.value === null) return;
  autostartBusy.value = true;
  autostartError.value = false;
  try {
    await ResidentService.setAutostart(enabled);
    autostart.value = enabled;
  } catch {
    autostartError.value = true;
    // The OS may have changed state before reporting failure; reconcile when possible.
    try {
      autostart.value = await ResidentService.autostartEnabled();
    } catch {
      /* Keep the last known state. */
    }
  } finally {
    autostartBusy.value = false;
  }
}
onMounted(loadAutostart);
</script>

<template>
  <div class="autostart-settings">
    <div class="setting-row grid-cols-[40px_minmax(0,1fr)_auto] @2xl/settings:grid-cols-[42px_minmax(0,1fr)_auto]">
      <span class="section-icon"><MdIcon :name="ICON_NAMES.startup" /></span>
      <label class="setting-copy" for="resident-autostart"
        ><strong>{{ t('monitoring.autostart') }}</strong
        ><small>{{ t('monitoring.autostartHint') }}</small></label
      >
      <MdSwitch
        id="resident-autostart"
        :model-value="autostart ?? false"
        :disabled="autostartBusy || autostart === null"
        @update:model-value="saveAutostart"
      />
    </div>
    <div v-if="autostartError" class="autostart-settings-error" role="alert">
      {{ t('monitoring.settingsFailed') }} <button @click="loadAutostart">{{ t('monitoring.refresh') }}</button>
    </div>
  </div>
</template>

<style scoped>
@reference "@assets/main.css";
.autostart-settings-error {
  @apply text-destructive;
  padding: 0 20px 16px;
  font-size: var(--font-content-secondary);
}
.autostart-settings-error button {
  text-decoration: underline;
  cursor: pointer;
}
</style>
