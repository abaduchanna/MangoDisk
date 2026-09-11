<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';

import MdIcon from '@/components/icons/md-icon.vue';
import MdMemoryOverview from './components/md-memory-overview.vue';
import MdApplicationMemoryList from './components/md-application-memory-list.vue';
import { ICON_NAMES } from '@/lib/models/ui';
import type { ResidentDestination } from '@/lib/models/resident';
import { ResidentService } from '@/lib/services/resident-service';
import { useTrayPanelStore } from '@/stores/tray-panel-store';
import { useAppStore } from '@/stores/app-store';

const { t } = useI18n({ useScope: 'global' });
const store = useTrayPanelStore();
const appStore = useAppStore();
const panel = ref<HTMLElement | null>(null);
// Feedback belongs to this panel's presentation lifecycle. Start its timeout only
// after loading ends; retrying or unmounting cancels the previous result's timer.
watch(
  [() => store.releasing, () => store.releaseResult],
  ([releasing, result], _previous, onCleanup) => {
    if (releasing || !result) return;
    const timer = setTimeout(() => {
      store.releaseResult = null;
    }, 3000);
    onCleanup(() => clearTimeout(timer));
  },
  { immediate: true }
);
let disposed = false;
const disposers: (() => void)[] = [];
function retain(dispose: () => void) {
  if (disposed) dispose();
  else disposers.push(dispose);
}

async function act(action: () => Promise<void>) {
  try {
    await action();
  } catch {
    store.fail('monitoring_action_failed');
  }
}
function navigate(destination: ResidentDestination) {
  void act(() => ResidentService.openMain(destination));
}
function onKey(event: KeyboardEvent) {
  if (event.key === 'Escape') {
    event.preventDefault();
    void act(() => ResidentService.hidePanel());
  }
}
let subscribed = false;
let connecting: Promise<void> | null = null;
async function connect() {
  if (subscribed || disposed) return;
  if (connecting) return connecting;
  connecting = (async () => {
    // Retrying reconnects the event stream; a one-off cached read cannot restore live updates.
    const pending: (() => void)[] = [];
    try {
      pending.push(
        await ResidentService.onReading(reading => {
          if (!disposed) store.accept(reading);
        })
      );
      if (!disposed)
        pending.push(
          await ResidentService.onFocus(() => {
            // A prewarmed WebView survives closing. Do not present an old result
            // as a new action's state when the user returns to the panel.
            if (!store.releasing) store.releaseResult = null;
            void appStore.loadSettings();
            panel.value?.focus({ preventScroll: true });
          })
        );
      pending.forEach(retain);
      subscribed = !disposed;
    } catch (error) {
      pending.forEach(dispose => dispose());
      throw error;
    }
  })();
  try {
    await connecting;
  } finally {
    connecting = null;
  }
}
async function refresh() {
  try {
    await connect();
    if (!disposed) await store.refresh();
  } catch {
    store.fail('monitoring_subscription_failed');
  }
}
onMounted(() => {
  window.addEventListener('keydown', onKey);
  // Reveal the first rendered frame independently of IPC, samples, and icons.
  // Native icon components progressively fill their placeholders using the shared cache.
  void act(() => ResidentService.panelReady()).then(() => {
    if (!disposed) panel.value?.focus({ preventScroll: true });
  });
  void connect()
    .then(() => {
      if (!disposed) return store.load();
    })
    .catch(() => {
      if (!disposed) store.fail('monitoring_subscription_failed');
    });
});
onBeforeUnmount(() => {
  disposed = true;
  window.removeEventListener('keydown', onKey);
  disposers.forEach(dispose => dispose());
});
</script>

<template>
  <main ref="panel" class="monitor-panel" tabindex="-1" :aria-label="t('monitoring.title')">
    <div class="monitor-body">
      <div v-if="store.error" class="monitor-notice" role="alert">
        {{ t('monitoring.unavailable') }} <button @click="refresh()">{{ t('monitoring.refresh') }}</button>
      </div>
      <template v-if="store.reading.snapshot">
        <MdMemoryOverview
          :memory="store.reading.snapshot.memory"
          :releasing="store.releasing"
          :release-result="store.releaseResult"
          @release="store.releaseMemory()"
        />
        <MdApplicationMemoryList class="monitor-processes" :summary="store.reading.snapshot.processes" />
      </template>
      <div v-else class="monitor-loading" role="status">
        {{ t(store.reading.status === 'paused' ? 'monitoring.paused' : 'monitoring.loading') }}
      </div>
    </div>
    <footer>
      <button class="panel-icon-button" :aria-label="t('monitoring.settings')" @click="navigate('settings')">
        <MdIcon :name="ICON_NAMES.settings" :size="16" />
      </button>
      <button class="open-main-shortcut" @click="navigate('main')">
        {{ t('monitoring.openMain') }}
      </button>
      <button class="quit-shortcut" @click="act(() => ResidentService.quit())">
        {{ t('monitoring.quit') }}
      </button>
    </footer>
  </main>
</template>

<style scoped>
@reference "@assets/main.css";
.monitor-panel {
  @apply bg-background text-foreground;
  display: flex;
  flex-direction: column;
  height: 100dvh;
  border: 1px solid var(--border);
  overflow: hidden;
}
.monitor-panel:focus {
  outline: none;
}
.panel-icon-button {
  @apply text-muted-foreground;
  display: inline-grid;
  place-items: center;
  width: 30px;
  height: 30px;
  border-radius: 7px;
  flex: none;
}
button {
  cursor: pointer;
}
button:hover {
  @apply bg-accent text-accent-foreground;
}
button:focus-visible {
  outline: 2px solid var(--ring);
  outline-offset: 2px;
}
button:disabled {
  cursor: default;
  opacity: 0.45;
}
.monitor-body {
  display: flex;
  flex-direction: column;
  gap: 14px;
  overflow: hidden;
  min-height: 0;
  flex: 1;
  padding: 12px 12px 14px;
}
.monitor-processes {
  /* Extend the scroll viewport through the body's right inset to the window edge. */
  margin-right: -12px;
}
.monitor-loading {
  @apply text-muted-foreground;
  display: grid;
  place-items: center;
  min-height: 200px;
  font-size: 12px;
}
.monitor-notice {
  @apply border border-border rounded-lg text-muted-foreground;
  flex: none;
  font-size: 11px;
  line-height: 1.5;
  padding: 10px;
}
.monitor-notice button {
  @apply text-primary;
  text-decoration: underline;
}
footer {
  @apply border-t border-border text-muted-foreground;
  display: grid;
  grid-template-columns: minmax(40px, 1fr) auto minmax(40px, 1fr);
  align-items: center;
  flex: none;
  padding: 6px 14px;
}
.quit-shortcut {
  justify-self: end;
}
.open-main-shortcut,
.quit-shortcut {
  /* Equal side columns keep the main action centered in every locale. */
  justify-content: center;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  border-radius: 7px;
  min-height: 30px;
  padding: 0 8px;
  font-size: 11px;
}
</style>
