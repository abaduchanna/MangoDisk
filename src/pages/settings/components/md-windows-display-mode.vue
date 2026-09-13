<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import MdSwitch from '@/components/custom/md-switch.vue';
import type { ResidentDisplayStatus, ResidentPreferences } from '@/lib/models/resident';
import { ResidentService } from '@/lib/services/resident-service';
import { LoggerService } from '@/lib/services/logger-service';
const props = defineProps<{ preferences: ResidentPreferences }>();
const emit = defineEmits<{
  change: [mode: ResidentPreferences['windowsDisplayMode']];
  position: [position: ResidentPreferences['taskbarPosition']];
  background: [enabled: boolean];
  compact: [enabled: boolean];
}>();
const { t } = useI18n({ useScope: 'global' });
const status = ref<ResidentDisplayStatus>('tray');
const hint = computed(() => {
  if (
    !props.preferences.enabled ||
    props.preferences.windowsDisplayMode !== 'taskbar' ||
    !props.preferences.metrics.some(item => item.enabled)
  )
    return null;
  return (
    (
      {
        noSpace: 'systemStatus.taskbarNoSpace',
        unsupportedLayout: 'systemStatus.taskbarUnsupported',
        shellUnavailable: 'systemStatus.taskbarUnavailable',
      } as Partial<Record<ResidentDisplayStatus, string>>
    )[status.value] ?? null
  );
});
let disposed = false;
let unlisten: (() => void) | undefined;
onMounted(async () => {
  try {
    let received = false;
    const stop = await ResidentService.onDisplayStatus(value => {
      received = true;
      if (!disposed) status.value = value;
    });
    if (disposed) {
      stop();
      return;
    }
    unlisten = stop;
    const value = await ResidentService.displayStatus();
    if (!disposed && !received) status.value = value;
  } catch {
    LoggerService.warn('resident', 'display_status_load_failed');
  }
});
onBeforeUnmount(() => {
  disposed = true;
  unlisten?.();
});
</script>
<template>
  <div class="windows-display-settings">
    <div class="display-field">
      <span id="windows-display-mode-label" class="field-label">{{ t('systemStatus.displayMode') }}</span>
      <div class="segmented-choice" role="radiogroup" aria-labelledby="windows-display-mode-label">
        <label v-for="mode in ['taskbar', 'tray'] as const" :key="mode">
          <input
            type="radio"
            name="windows-display-mode"
            :value="mode"
            :checked="preferences.windowsDisplayMode === mode"
            @change="emit('change', mode)"
          />
          <span>{{ t(mode === 'taskbar' ? 'systemStatus.taskbarMode' : 'systemStatus.trayMode') }}</span>
        </label>
      </div>
    </div>
    <div v-if="preferences.windowsDisplayMode === 'taskbar'" class="taskbar-options">
      <div class="display-field">
        <span id="taskbar-position-label" class="field-label">{{ t('systemStatus.taskbarPosition') }}</span>
        <div class="segmented-choice" role="radiogroup" aria-labelledby="taskbar-position-label">
          <label v-for="position in ['auto', 'left', 'right'] as const" :key="position">
            <input
              type="radio"
              name="taskbar-position"
              :value="position"
              :checked="preferences.taskbarPosition === position"
              @change="emit('position', position)"
            />
            <span>{{
              t(
                position === 'auto'
                  ? 'systemStatus.taskbarAuto'
                  : position === 'left'
                    ? 'systemStatus.taskbarLeft'
                    : 'systemStatus.taskbarRight'
              )
            }}</span>
          </label>
        </div>
      </div>
      <div class="background-field">
        <label for="taskbar-compact">{{ t('systemStatus.taskbarCompact') }}</label>
        <MdSwitch
          id="taskbar-compact"
          :model-value="preferences.taskbarCompact"
          @update:model-value="emit('compact', $event)"
        />
      </div>
      <div class="background-field">
        <label for="taskbar-background">{{ t('systemStatus.taskbarBackground') }}</label>
        <MdSwitch
          id="taskbar-background"
          :model-value="preferences.taskbarBackground"
          @update:model-value="emit('background', $event)"
        />
      </div>
    </div>
    <p v-if="hint" role="status" class="text-muted-foreground text-xs">{{ t(hint) }}</p>
  </div>
</template>

<style scoped>
@reference "@assets/main.css";
.windows-display-settings {
  display: grid;
  gap: 12px;
  font-size: var(--font-content-secondary);
}
.display-field,
.background-field {
  display: flex;
  align-items: center;
  gap: 12px;
}
.display-field {
  flex-wrap: wrap;
}
.field-label {
  min-width: 6rem;
}
.taskbar-options {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 12px 24px;
}
.segmented-choice {
  @apply bg-muted rounded-lg;
  display: inline-flex;
  padding: 3px;
  gap: 2px;
  max-width: 100%;
}
.segmented-choice label {
  position: relative;
  flex: 1;
  cursor: pointer;
}
.segmented-choice input {
  @apply sr-only;
}
.segmented-choice span {
  @apply text-muted-foreground rounded-md;
  display: block;
  min-width: 4rem;
  padding: 5px 12px;
  text-align: center;
  white-space: nowrap;
  line-height: 20px;
  transition:
    color 150ms,
    background-color 150ms,
    box-shadow 150ms;
}
.segmented-choice label:hover span {
  @apply text-foreground;
}
/* Native radios retain arrow-key navigation and a single Tab stop. Keep the
   selected surface neutral so this setting does not compete with resource chips. */
.segmented-choice input:checked + span {
  @apply bg-card text-primary shadow-sm;
}
.segmented-choice input:focus-visible + span {
  outline: 2px solid var(--ring);
  outline-offset: 1px;
}
@media (prefers-reduced-motion: reduce) {
  .segmented-choice span {
    transition: none;
  }
}
</style>
