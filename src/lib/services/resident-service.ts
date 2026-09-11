import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';

import type {
  ApplicationQuitStatus,
  MemoryReleaseResult,
  ResidentDestination,
  ResidentPreferences,
  ResidentReading,
} from '@/lib/models/resident';

export class ResidentService {
  static reading(): Promise<ResidentReading> {
    return invoke('monitoring_get_reading');
  }
  static refresh(): Promise<void> {
    return invoke('monitoring_refresh');
  }
  static releaseMemory(): Promise<MemoryReleaseResult> {
    return invoke('monitoring_release_memory');
  }
  static quitApplication(applicationId: string): Promise<ApplicationQuitStatus> {
    return invoke('monitoring_quit_application', { applicationId });
  }
  static preferences(): Promise<ResidentPreferences> {
    return invoke('resident_get_preferences');
  }
  static savePreferences(preferences: ResidentPreferences): Promise<void> {
    return invoke('resident_save_preferences', { preferences });
  }
  static autostartEnabled(): Promise<boolean> {
    return invoke('resident_get_autostart');
  }
  static setAutostart(enabled: boolean): Promise<void> {
    return invoke('resident_set_autostart', { enabled });
  }
  static openPanel(): Promise<void> {
    return invoke('resident_open_panel');
  }
  static panelReady(): Promise<void> {
    return invoke('resident_panel_ready');
  }
  static hidePanel(): Promise<void> {
    return invoke('resident_hide_panel');
  }
  static openMain(destination: ResidentDestination): Promise<void> {
    return invoke('resident_open_main', { destination });
  }
  static quit(): Promise<void> {
    return invoke('resident_quit');
  }
  static onReading(handler: (reading: ResidentReading) => void): Promise<UnlistenFn> {
    return listen<ResidentReading>('resident-reading', event => handler(event.payload));
  }
  static onNavigate(handler: (destination: ResidentDestination) => void): Promise<UnlistenFn> {
    return listen<ResidentDestination>('resident-open-page', event => handler(event.payload));
  }
  static onFocus(handler: () => void): Promise<UnlistenFn> {
    return getCurrentWindow().onFocusChanged(event => {
      if (event.payload) handler();
    });
  }
}
