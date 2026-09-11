export interface MemoryOverview {
  totalBytes: number;
  usedBytes: number;
  freeBytes: number;
  swapUsedBytes: number;
  usedPercent: number;
}

export interface ApplicationMemory {
  id: string;
  name: string;
  residentBytes: number;
  processCount: number;
  iconPath: string | null;
  isBundle: boolean;
  canQuit: boolean;
}

export interface ProcessMemorySummary {
  applications: ApplicationMemory[];
  readableProcessCount: number;
  omittedProcessCount: number;
}

export interface SystemResourceSnapshot {
  schemaVersion: 1;
  sampledAtMs: number;
  memory: MemoryOverview;
  processes: ProcessMemorySummary | null;
}

export interface ResidentReading {
  revision: number;
  status: 'loading' | 'ready' | 'unavailable' | 'paused';
  snapshot: SystemResourceSnapshot | null;
}

export interface ResidentPreferences {
  schemaVersion: 1;
  enabled: boolean;
  showMemory: boolean;
}

export type ResidentDestination = 'main' | 'cleanup' | 'applications' | 'settings' | 'about';

export interface MemoryReleaseResult {
  schemaVersion: 1;
  status: 'completed' | 'cancelled' | 'unsupported' | 'failed' | 'busy';
  observedReductionBytes: number | null;
}

export type ApplicationQuitStatus = 'requested' | 'unavailable' | 'unsupported';
