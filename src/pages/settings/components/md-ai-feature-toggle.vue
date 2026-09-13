<script setup lang="ts">
import { useI18n } from 'vue-i18n';
import { Button } from '@/components/ui/button';
import MdSwitch from '@/components/custom/md-switch.vue';
import MdIcon from '@/components/icons/md-icon.vue';
import { ICON_NAMES } from '@/lib/models/ui';
import { useAiStore } from '@/stores/ai-store';

const { t } = useI18n({ useScope: 'global' });
const ai = useAiStore();
const emit = defineEmits<{ configure: [] }>();
</script>

<template>
  <div class="ai-feature-toggle" :aria-busy="ai.preferencesBusy">
    <div class="setting-row grid-cols-[40px_minmax(0,1fr)_auto] @2xl/settings:grid-cols-[42px_minmax(0,1fr)_auto]">
      <span class="section-icon"><MdIcon :name="ICON_NAMES.sparkles" /></span>
      <div class="setting-copy">
        <strong>{{ t('ai.providerTitle') }}</strong>
        <small id="ai-enabled-hint" class="whitespace-normal!">{{
          t(ai.enabled || !ai.preferencesLoaded ? 'ai.settingsDescription' : 'ai.featureDisabledHint')
        }}</small>
      </div>
      <div class="flex shrink-0 items-center gap-3">
        <Button
          v-if="ai.enabled"
          variant="ghost"
          size="sm"
          class="text-muted-foreground"
          :disabled="ai.preferencesBusy"
          aria-haspopup="dialog"
          @click="emit('configure')"
        >
          {{ t('ai.configureAction') }}
        </Button>
        <MdSwitch
          id="ai-enabled"
          :model-value="ai.enabled"
          :disabled="ai.preferencesBusy || !ai.preferencesLoaded"
          :aria-label="t('ai.enableFeature')"
          aria-describedby="ai-enabled-hint"
          @update:model-value="ai.setEnabled"
        />
      </div>
    </div>
    <div v-if="ai.preferencesError" class="px-5 pb-4 text-sm text-destructive" role="alert">
      {{ t(ai.preferencesError === 'load' ? 'ai.featureLoadFailed' : 'ai.featureSaveFailed') }}
      <button
        v-if="ai.preferencesError === 'load'"
        type="button"
        class="cursor-pointer underline"
        :disabled="ai.preferencesBusy"
        @click="ai.loadPreferences"
      >
        {{ t('ai.retry') }}
      </button>
    </div>
  </div>
</template>
