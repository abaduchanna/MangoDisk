<script setup lang="ts">
import { METRIC_LABEL_KEYS } from '@/lib/models/system-resources';
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import MdSwitch from '@/components/custom/md-switch.vue';
import MdIcon from '@/components/icons/md-icon.vue';
import MdIconMangodisk from '@/components/icons/md-icon-mangodisk.vue';
import { Card } from '@/components/ui/card';
import { Select, SelectContent, SelectItem } from '@/components/ui/select';
import { SelectTrigger } from 'reka-ui';
import MdWindowsDisplayMode from './md-windows-display-mode.vue';
import { ICON_NAMES } from '@/lib/models/ui';
import type { MetricId, NetworkInterface, ResourceVolume } from '@/lib/models/system-resources';
import type { ResidentPreferences, ResidentReading } from '@/lib/models/resident';
import { ResidentService } from '@/lib/services/resident-service';
import { useResidentSettingsStore } from '@/stores/resident-settings-store';

const props = defineProps<{ isMacOs: boolean }>();
const { t } = useI18n({ useScope: 'global' });
const settings = useResidentSettingsStore();
const interfaces = ref<NetworkInterface[]>([]);
const volumes = ref<ResourceVolume[]>([]);
const catalogueError = ref(false);
const dragging = ref<MetricId | null>(null);
const pointerDragging = ref(false);
const dragRows = ref<ResidentPreferences['metrics'] | null>(null);
const announcement = ref('');
const metricRowsElement = ref<HTMLElement | null>(null);
const canReorder = computed(() => props.isMacOs || settings.draft?.windowsDisplayMode === 'taskbar');
const rows = computed(() => dragRows.value ?? settings.draft?.metrics ?? []);
// Older preferences may have every item cleared. Mirror the native Logo
// fallback without writing on load, and retain it when the next metric is added.
const showIcon = computed(() => (settings.draft?.showIcon ?? true) || !rows.value.some(row => row.enabled));
const selectedCount = computed(() => Number(showIcon.value) + rows.value.filter(row => row.enabled).length);
function selectionLocked(selected: boolean) {
  return !settings.draft || (selected && selectedCount.value <= 1);
}
function enableIcon(enabled: boolean) {
  if (!enabled && selectionLocked(showIcon.value)) return;
  void settings.change({ showIcon: enabled });
}
const expandedSelection = ref<'network' | 'disk' | null>(null);
// Keep the native switch and collapsed content on the committed state. Failed
// writes must not hide the user's controls or leave a misleading enabled state.
const displayEnabled = computed(() => settings.preferences?.enabled ?? false);
async function setDisplayEnabled(enabled: boolean) {
  cancelDrag();
  expandedSelection.value = null;
  await settings.change({ enabled });
}
watch(displayEnabled, enabled => {
  if (!enabled) {
    cancelDrag();
    expandedSelection.value = null;
  }
});
function setSelectionOpen(id: MetricId, open: boolean) {
  if (id !== 'network' && id !== 'disk') return;
  if (open) expandedSelection.value = id;
  else if (expandedSelection.value === id) expandedSelection.value = null;
}
watch(rows, value => {
  if (expandedSelection.value && !value.some(row => row.id === expandedSelection.value && row.enabled)) {
    expandedSelection.value = null;
  }
});
const missingInterface = computed(
  () => settings.draft?.networkInterface && !interfaces.value.some(item => item.id === settings.draft?.networkInterface)
);
const missingVolume = computed(
  () => settings.draft?.diskVolume && !volumes.value.some(item => item.id === settings.draft?.diskVolume)
);
// Select's inferred item text can outlive a locale or catalogue update while
// its popup is closed. Keep the displayed selection reactive without reopening it.
const selectedInterfaceLabel = computed(() => {
  const id = settings.draft?.networkInterface;
  if (!id) return t('systemStatus.automatic');
  const item = interfaces.value.find(item => item.id === id);
  if (!item) return t('systemStatus.savedDisconnected');
  return `${item.name}${item.connected ? '' : ` · ${t('systemStatus.disconnected')}`}`;
});
const selectedVolumeLabel = computed(() => {
  const id = settings.draft?.diskVolume;
  if (!id) return t('systemStatus.systemDisk');
  return volumes.value.find(item => item.id === id)?.name ?? t('systemStatus.savedDisconnected');
});
let disposed = false;
let unlisten: (() => void) | null = null;
let revision = -1;
function accept(value: ResidentReading) {
  if (disposed || value.schemaVersion !== 3 || value.revision < revision) return;
  revision = value.revision;
  interfaces.value = value.interfaces;
  volumes.value = value.volumes;
}
function enable(id: MetricId, enabled: boolean) {
  if (!enabled && selectionLocked(rows.value.some(row => row.id === id && row.enabled))) return;
  cancelDrag();
  void settings.change({
    showIcon: showIcon.value,
    metrics: rows.value.map(metric => (metric.id === id ? { ...metric, enabled } : { ...metric })),
  });
}
function begin(id: MetricId) {
  if (!canReorder.value || !settings.draft) return;
  dragging.value = id;
  dragRows.value = settings.draft.metrics.map(metric => ({ ...metric }));
  announce();
}
function move(target: MetricId) {
  if (!dragRows.value || !dragging.value || target === dragging.value) return;
  const from = dragRows.value.findIndex(row => row.id === dragging.value);
  const to = dragRows.value.findIndex(row => row.id === target);
  if (from < 0 || to < 0) return;
  const next = [...dragRows.value];
  const [item] = next.splice(from, 1);
  if (item) next.splice(to, 0, item);
  dragRows.value = next;
  if (!pointer) restoreHandleFocus(dragging.value);
  announce();
}
function restoreHandleFocus(id: MetricId) {
  // WebKit drops focus when Vue moves a keyed row. Restore it after the DOM
  // update so subsequent arrows, Enter and Escape still reach the same handle.
  void nextTick(() =>
    metricRowsElement.value
      ?.querySelector<HTMLButtonElement>(`[data-metric="${id}"] .drag-handle`)
      ?.focus({ preventScroll: true })
  );
}
function announce() {
  announcement.value = t('systemStatus.moveAnnouncement', {
    name: t(METRIC_LABEL_KEYS[dragging.value!]),
    position: rows.value.findIndex(row => row.id === dragging.value) + 1,
    count: rows.value.length,
  });
}
let pointer: {
  id: MetricId;
  pointerId: number;
  x: number;
  y: number;
  root: HTMLElement;
  row: HTMLElement;
  bounds: DOMRect;
  slots: DOMRect[];
} | null = null;
let dragPreview: HTMLElement | null = null;
function createDragPreview() {
  if (!pointer) return;
  // Clone only the presentation. The inert copy has no duplicate IDs, focusable
  // controls or accessible content; the original row remains the drop placeholder.
  dragPreview = pointer.row.cloneNode(true) as HTMLElement;
  dragPreview.classList.add('drag-preview');
  dragPreview.classList.remove('moving');
  dragPreview.inert = true;
  dragPreview.setAttribute('aria-hidden', 'true');
  dragPreview.removeAttribute('data-metric');
  dragPreview.querySelectorAll('[id]').forEach(element => element.removeAttribute('id'));
  dragPreview.querySelectorAll('label').forEach(element => element.removeAttribute('for'));
  Object.assign(dragPreview.style, {
    left: `${pointer.bounds.left}px`,
    top: `${pointer.bounds.top}px`,
    width: `${pointer.bounds.width}px`,
    height: `${pointer.bounds.height}px`,
  });
  document.body.append(dragPreview);
  pointerDragging.value = true;
}
function escapePointerDrag(event: KeyboardEvent) {
  if (event.key === 'Escape') {
    event.preventDefault();
    cancelDrag();
  }
}
function pointerDown(event: PointerEvent, id: MetricId) {
  if (!canReorder.value || event.button !== 0) return;
  cancelDrag();
  const handle = event.currentTarget as HTMLElement;
  const root = handle.closest<HTMLElement>('.metric-rows');
  const row = handle.closest<HTMLElement>('.metric-row');
  if (!root || !row) return;
  event.preventDefault();
  expandedSelection.value = null;
  pointer = {
    id,
    pointerId: event.pointerId,
    x: event.clientX,
    y: event.clientY,
    root,
    row,
    bounds: row.getBoundingClientRect(),
    // Hit-test stable grid slots, not the animated cards passing under the pointer.
    // This also handles wrapped rows without oscillating between adjacent items.
    slots: [...root.querySelectorAll<HTMLElement>('.metric-row')].map(item => item.getBoundingClientRect()),
  };
  // Native Tauri file drops must stay enabled for cleanup pages. Pointer events
  // keep this local reorder independent of the intercepted HTML drag pipeline.
  window.addEventListener('pointermove', pointerMove);
  window.addEventListener('pointerup', pointerUp);
  window.addEventListener('pointercancel', cancelDrag);
  window.addEventListener('blur', cancelDrag);
  window.addEventListener('keydown', escapePointerDrag);
  window.addEventListener('resize', cancelDrag);
  window.addEventListener('scroll', cancelDrag, true);
}
function pointerMove(event: PointerEvent) {
  if (!pointer || event.pointerId !== pointer.pointerId) return;
  if (!dragging.value && Math.hypot(event.clientX - pointer.x, event.clientY - pointer.y) >= 4) {
    createDragPreview();
    begin(pointer.id);
  }
  if (!dragging.value) return;
  const dx = event.clientX - pointer.x;
  const dy = event.clientY - pointer.y;
  if (dragPreview) dragPreview.style.transform = `translate3d(${dx}px, ${dy}px, 0)`;
  const x = pointer.bounds.left + pointer.bounds.width / 2 + dx;
  const y = pointer.bounds.top + pointer.bounds.height / 2 + dy;
  const index = pointer.slots.findIndex(slot => x >= slot.left && x <= slot.right && y >= slot.top && y <= slot.bottom);
  const target = rows.value[index];
  if (target) move(target.id);
}
function pointerUp(event: PointerEvent) {
  if (!pointer || event.pointerId !== pointer.pointerId) return;
  const target = document.elementFromPoint(event.clientX, event.clientY);
  if (dragging.value && target && pointer.root.contains(target)) commit();
  else cancelDrag();
}
function cancelDrag() {
  if (dragging.value && !pointer) restoreHandleFocus(dragging.value);
  dragPreview?.remove();
  dragPreview = null;
  pointerDragging.value = false;
  pointer = null;
  window.removeEventListener('pointermove', pointerMove);
  window.removeEventListener('pointerup', pointerUp);
  window.removeEventListener('pointercancel', cancelDrag);
  window.removeEventListener('blur', cancelDrag);
  window.removeEventListener('keydown', escapePointerDrag);
  window.removeEventListener('resize', cancelDrag);
  window.removeEventListener('scroll', cancelDrag, true);
  dragging.value = null;
  dragRows.value = null;
}
function commit() {
  const metrics = dragRows.value;
  cancelDrag();
  if (metrics && metrics.some((row, index) => row.id !== settings.draft?.metrics[index]?.id)) {
    void settings.change({ metrics });
  }
}
function key(event: KeyboardEvent, id: MetricId) {
  if (!canReorder.value) return;
  if (event.key === 'Escape') {
    event.preventDefault();
    cancelDrag();
  } else if (event.key === ' ' || event.key === 'Enter') {
    event.preventDefault();
    if (dragging.value) commit();
    else begin(id);
  } else if (dragging.value && ['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight'].includes(event.key)) {
    event.preventDefault();
    const index = rows.value.findIndex(row => row.id === dragging.value);
    // Left/right follows the compact item order; retain up/down for existing keyboard users.
    const next = rows.value[index + (event.key === 'ArrowUp' || event.key === 'ArrowLeft' ? -1 : 1)];
    if (next) move(next.id);
  }
}
async function loadCatalogue() {
  catalogueError.value = false;
  try {
    // A retry must restore the live subscription as well as the cached catalogue.
    if (!unlisten) {
      const dispose = await ResidentService.onReading(accept);
      if (disposed) {
        dispose();
        return;
      }
      unlisten = dispose;
    }
    accept(await ResidentService.catalogue());
  } catch {
    if (!disposed) catalogueError.value = true;
  }
}
onMounted(() => {
  if (!settings.preferences) void settings.load();
  void loadCatalogue();
});
onBeforeUnmount(() => {
  disposed = true;
  cancelDrag();
  unlisten?.();
});
</script>

<template>
  <section class="status-settings" :class="{ windows: !isMacOs }">
    <h2>{{ t(isMacOs ? 'systemStatus.menuBarTitle' : 'systemStatus.trayTitle') }}</h2>
    <Card class="status-card">
      <div class="display-toggle-row">
        <span class="display-toggle-icon" aria-hidden="true"
          ><MdIcon :name="isMacOs ? ICON_NAMES.menuBar : ICON_NAMES.taskbar"
        /></span>
        <label for="resident-enabled" class="display-toggle-copy">
          <strong>{{ t(isMacOs ? 'systemStatus.menuBarEnabled' : 'systemStatus.displayEnabled') }}</strong>
          <small>{{ t(isMacOs ? 'systemStatus.menuBarHint' : 'systemStatus.displayHint') }}</small>
        </label>
        <MdSwitch
          id="resident-enabled"
          :model-value="displayEnabled"
          :disabled="settings.loading || settings.saving || !settings.preferences"
          aria-controls="resident-display-options"
          @update:model-value="setDisplayEnabled"
        />
      </div>
      <div v-if="displayEnabled" id="resident-display-options" class="display-options">
        <MdWindowsDisplayMode
          v-if="!isMacOs && settings.draft"
          :preferences="settings.draft"
          @position="settings.change({ taskbarPosition: $event })"
          @background="settings.change({ taskbarBackground: $event })"
          @compact="settings.change({ taskbarCompact: $event })"
          @change="
            cancelDrag();
            settings.change({ windowsDisplayMode: $event });
          "
        />
        <div class="status-controls">
          <div class="controls-heading">
            <span>{{ t('systemStatus.displayItems') }}</span>
            <span v-if="canReorder">{{ t('systemStatus.dragToReorder') }}</span>
          </div>
          <div ref="metricRowsElement" @keydown.esc="cancelDrag">
            <TransitionGroup name="metric-sort" tag="div" class="metric-rows">
              <div key="logo" class="status-item" :class="{ selected: showIcon }">
                <label class="logo-label" for="status-app-icon">
                  <input
                    id="status-app-icon"
                    type="checkbox"
                    :checked="showIcon"
                    :disabled="selectionLocked(showIcon)"
                    @change="enableIcon(($event.target as HTMLInputElement).checked)"
                  />
                  <MdIconMangodisk :size="18" class="shrink-0" />
                  {{ t('systemStatus.showIcon') }}
                </label>
              </div>
              <div
                v-for="row in rows"
                :key="row.id"
                class="status-item metric-row"
                :class="{
                  moving: dragging === row.id,
                  'pointer-moving': pointerDragging && dragging === row.id,
                  selected: row.enabled,
                }"
                :data-metric="row.id"
              >
                <div class="metric-heading">
                  <button
                    v-if="canReorder"
                    class="drag-handle"
                    :aria-label="t('systemStatus.reorder', { name: t(METRIC_LABEL_KEYS[row.id]) })"
                    :aria-pressed="dragging === row.id"
                    @pointerdown="pointerDown($event, row.id)"
                    @dragstart.prevent
                    @keydown="key($event, row.id)"
                  >
                    <MdIcon :name="ICON_NAMES.grip" :size="15" />
                  </button>
                  <label :for="`status-${row.id}`">
                    <input
                      :id="`status-${row.id}`"
                      type="checkbox"
                      :checked="row.enabled"
                      :disabled="selectionLocked(row.enabled)"
                      @change="enable(row.id, ($event.target as HTMLInputElement).checked)"
                    />
                    {{
                      row.id === 'cpu'
                        ? t('systemStatus.cpuShort')
                        : row.id === 'disk'
                          ? t('systemStatus.volume')
                          : t(METRIC_LABEL_KEYS[row.id])
                    }}
                  </label>
                  <Select
                    v-if="row.id === 'network' || row.id === 'disk'"
                    :disabled="!row.enabled"
                    :open="expandedSelection === row.id"
                    :model-value="
                      row.id === 'network'
                        ? (settings.draft?.networkInterface ?? 'automatic')
                        : (settings.draft?.diskVolume ?? 'system')
                    "
                    @update:open="setSelectionOpen(row.id, $event)"
                    @update:model-value="
                      row.id === 'network'
                        ? settings.change({ networkInterface: $event === 'automatic' ? null : String($event) })
                        : settings.change({ diskVolume: $event === 'system' ? null : String($event) })
                    "
                  >
                    <SelectTrigger as-child>
                      <button
                        type="button"
                        class="selection-toggle"
                        :aria-label="
                          row.id === 'network'
                            ? `${t('systemStatus.interface')}: ${selectedInterfaceLabel}`
                            : `${t('systemStatus.volume')}: ${selectedVolumeLabel}`
                        "
                        :title="row.id === 'network' ? selectedInterfaceLabel : selectedVolumeLabel"
                      >
                        <MdIcon
                          :name="expandedSelection === row.id ? ICON_NAMES.chevronUp : ICON_NAMES.chevronDown"
                          :size="14"
                        />
                      </button>
                    </SelectTrigger>
                    <SelectContent align="end" class="max-w-[min(20rem,calc(100vw-2rem))]">
                      <template v-if="row.id === 'network'">
                        <SelectItem value="automatic">{{ t('systemStatus.automatic') }}</SelectItem>
                        <SelectItem v-for="item in interfaces" :key="item.id" :value="item.id" class="break-all">
                          {{ item.name }}{{ item.connected ? '' : ` · ${t('systemStatus.disconnected')}` }}
                        </SelectItem>
                        <SelectItem v-if="missingInterface" :value="settings.draft!.networkInterface!">{{
                          t('systemStatus.savedDisconnected')
                        }}</SelectItem>
                      </template>
                      <template v-else>
                        <SelectItem value="system">{{ t('systemStatus.systemDisk') }}</SelectItem>
                        <SelectItem v-for="item in volumes" :key="item.id" :value="item.id" class="break-all">{{
                          item.name
                        }}</SelectItem>
                        <SelectItem v-if="missingVolume" :value="settings.draft!.diskVolume!">{{
                          t('systemStatus.savedDisconnected')
                        }}</SelectItem>
                      </template>
                    </SelectContent>
                  </Select>
                </div>
              </div>
            </TransitionGroup>
          </div>
          <span class="sr-only" aria-live="polite">{{ announcement }}</span>
        </div>
      </div>
      <p v-if="settings.error || (displayEnabled && catalogueError)" class="settings-feedback" role="status">
        <template v-if="settings.error"
          >{{ t('systemStatus.saveFailed') }}
          <button @click="settings.load()">{{ t('monitoring.refresh') }}</button></template
        ><template v-else-if="catalogueError"
          >{{ t('systemStatus.catalogueFailed') }}
          <button @click="loadCatalogue">{{ t('monitoring.refresh') }}</button></template
        >
      </p>
    </Card>
  </section>
</template>

<style scoped>
@reference "@assets/main.css";
.status-settings h2 {
  @apply text-muted-foreground;
  font-size: 12px;
  font-weight: 600;
  margin: 0 0 10px 2px;
}
.status-card {
  gap: 12px;
  padding: 14px 16px;
}
.display-toggle-row {
  display: grid;
  grid-template-columns: 40px minmax(0, 1fr) auto;
  align-items: center;
  gap: 10px;
}
.display-toggle-icon {
  @apply text-muted-foreground;
  display: grid;
  place-items: center;
  width: 34px;
  height: 34px;
}
.display-toggle-copy {
  display: grid;
  gap: 4px;
  min-width: 0;
  cursor: pointer;
}
.display-toggle-copy strong {
  font-size: var(--font-content-primary);
  font-weight: 600;
}
.display-toggle-copy small {
  @apply text-muted-foreground;
  font-size: var(--font-content-secondary);
  line-height: 1.5;
}
.display-options {
  @apply border-t border-border;
  display: grid;
  gap: 12px;
  padding-top: 12px;
  min-width: 0;
}
.windows .display-options {
  /* Align child settings with the main label, leaving the icon in its own lane. */
  padding-left: 50px;
}
.controls-heading {
  @apply text-muted-foreground;
  display: flex;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: 4px 12px;
  margin-bottom: 8px;
  font-size: 12px;
}
.metric-rows {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(min(100%, 7.5rem), 1fr));
  gap: 8px;
}
.status-item {
  @apply border border-border/50 rounded-md bg-transparent;
  position: relative;
  isolation: isolate;
  display: flex;
  align-items: center;
  min-width: 0;
  min-height: 40px;
  padding: 2px 6px;
}
.status-item.selected {
  @apply border-transparent;
}
.status-item.selected::before,
.metric-row.pointer-moving::before {
  /* A separate paint layer keeps the tint subtle on Monterey, where Tailwind's
     color-mix fallback becomes opaque. Text and controls retain full contrast. */
  content: '';
  position: absolute;
  inset: 0;
  z-index: -1;
  border-radius: inherit;
  background: var(--accent);
  opacity: 0.15;
  pointer-events: none;
}
.metric-row.moving {
  outline: 2px solid var(--ring);
  outline-offset: 1px;
}
.metric-sort-move {
  transition: transform 200ms cubic-bezier(0.2, 0.8, 0.2, 1);
}
.metric-row.pointer-moving {
  @apply border-primary/30;
  border-style: dashed;
  outline: none;
  /* Keep the destination visible immediately; only neighbouring cards make way. */
  transition: none;
}
.pointer-moving .metric-heading {
  visibility: hidden;
}
.status-item.drag-preview {
  @apply bg-card border-primary/40 shadow-lg;
  position: fixed;
  z-index: 100;
  margin: 0;
  pointer-events: none;
  will-change: transform;
  cursor: grabbing;
}
@media (prefers-reduced-motion: reduce) {
  .metric-sort-move {
    transition: none;
  }
}
.metric-heading {
  display: flex;
  align-items: center;
  width: 100%;
  min-width: 0;
  gap: 2px;
}
.status-item label {
  display: flex;
  align-items: center;
  flex: 1;
  gap: 7px;
  min-width: 0;
  min-height: 32px;
  font-size: 12px;
  font-weight: 500;
  cursor: pointer;
  overflow-wrap: anywhere;
}
.logo-label {
  padding-inline: 6px;
}
.status-item input {
  width: 14px;
  height: 14px;
  flex: none;
  accent-color: var(--primary);
  cursor: pointer;
}
.status-item input:disabled {
  cursor: default;
}
.drag-handle,
.selection-toggle {
  @apply text-muted-foreground rounded;
  display: grid;
  place-items: center;
  flex: none;
  width: 24px;
  height: 32px;
  cursor: pointer;
}
.drag-handle {
  touch-action: none;
  user-select: none;
  cursor: grab;
}
.drag-handle:active {
  cursor: grabbing;
}
.drag-handle:focus-visible,
.selection-toggle:focus-visible {
  outline: 2px solid var(--ring);
}
.drag-handle:hover,
.selection-toggle:hover {
  @apply bg-accent text-foreground;
}
.selection-toggle[aria-expanded='true'] {
  @apply text-primary;
}
.selection-toggle:disabled {
  opacity: 0.4;
  cursor: default;
}
@media (pointer: coarse) {
  .metric-rows {
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 10rem), 1fr));
  }
  .status-item label,
  .drag-handle,
  .selection-toggle {
    min-height: 44px;
  }
  .drag-handle,
  .selection-toggle {
    width: 44px;
  }
}
.settings-feedback {
  @apply text-destructive;
  padding-top: 8px;
  font-size: 11px;
  line-height: 1.5;
}
.settings-feedback button {
  text-decoration: underline;
  cursor: pointer;
}
</style>
