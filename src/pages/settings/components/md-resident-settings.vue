<script setup lang="ts">
import { onMounted, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import MdSwitch from '@/components/custom/md-switch.vue';
import MdIcon from '@/components/icons/md-icon.vue';
import { ICON_NAMES } from '@/lib/models/ui';
import type { ResidentPreferences } from '@/lib/models/resident';
import { ResidentService } from '@/lib/services/resident-service';

const { t } = useI18n({ useScope: 'global' });
const preferences = ref<ResidentPreferences | null>(null);
const autostart = ref<boolean | null>(null);
const residentBusy = ref(false);
const autostartBusy = ref(false);
const residentError = ref(false);
const autostartError = ref(false);

async function loadResident() {
  residentBusy.value = true;
  residentError.value = false;
  try {
    preferences.value = await ResidentService.preferences();
  } catch {
    residentError.value = true;
  } finally {
    residentBusy.value = false;
  }
}
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
async function load() {
  // Independent OS and app settings retain their own result on partial failure.
  await Promise.all([loadResident(), loadAutostart()]);
}
async function save(value: boolean) {
  if (!preferences.value || residentBusy.value) return;
  residentBusy.value = true;
  residentError.value = false;
  const updated = { ...preferences.value, enabled: value };
  try {
    await ResidentService.savePreferences(updated);
    preferences.value = updated;
  } catch {
    residentError.value = true;
  } finally {
    residentBusy.value = false;
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
onMounted(load);
</script>

<template>
  <div class="resident-settings">
    <div class="setting-row grid-cols-[40px_minmax(0,1fr)_auto] @2xl/settings:grid-cols-[42px_minmax(0,1fr)_auto]">
      <span class="section-icon"><MdIcon :name="ICON_NAMES.application" /></span>
      <label class="setting-copy" for="resident-enabled"
        ><strong>{{ t('monitoring.enabled') }}</strong
        ><small>{{ t('monitoring.enabledHint') }}</small></label
      >
      <MdSwitch
        id="resident-enabled"
        :model-value="preferences?.enabled ?? false"
        :disabled="residentBusy || !preferences"
        @update:model-value="save($event)"
      />
    </div>
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
    <div v-if="residentError || autostartError" class="resident-settings-error" role="alert">
      {{ t('monitoring.settingsFailed') }} <button @click="load">{{ t('monitoring.refresh') }}</button>
    </div>
  </div>
</template>

<style scoped>
@reference "@assets/main.css";
.resident-settings-error {
  @apply text-destructive;
  padding: 0 20px 16px;
  font-size: var(--font-content-secondary);
}
.resident-settings-error button {
  text-decoration: underline;
  cursor: pointer;
}
</style>
