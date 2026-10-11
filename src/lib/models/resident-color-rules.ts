import type { MetricId } from '@/lib/models/system-resources';

export type ColorMetricId = Exclude<MetricId, 'network'> | 'cpuTemperature' | 'gpuTemperature';
export type DisplayColor = 'default' | 'green' | 'yellow' | 'orange' | 'red' | 'blue';
export type ColorRule =
  | { condition: 'below'; value: number; color: DisplayColor }
  | { condition: 'range'; from: number; to: number; color: DisplayColor }
  | { condition: 'atLeast'; value: number; color: DisplayColor };
export interface MetricColorRule {
  id: ColorMetricId;
  enabled: boolean;
  rules: ColorRule[];
}
export const MAX_COLOR_RULES = 16;
export const DISPLAY_COLORS: DisplayColor[] = ['default', 'green', 'yellow', 'orange', 'red', 'blue'];
export function isTemperatureMetric(id: ColorMetricId): boolean {
  return id === 'cpuTemperature' || id === 'gpuTemperature';
}
export function defaultColorRule(id: ColorMetricId): MetricColorRule {
  const [middle, high] = isTemperatureMetric(id) ? [60, 80] : [70, 90];
  return {
    id,
    enabled: true,
    rules: [
      { condition: 'below', value: middle!, color: 'green' },
      { condition: 'range', from: middle!, to: high!, color: 'orange' },
      { condition: 'atLeast', value: high!, color: 'red' },
    ],
  };
}
export function defaultColorRules(): MetricColorRule[] {
  return (['cpu', 'cpuTemperature', 'gpu', 'gpuTemperature', 'memory', 'disk'] as const).map(defaultColorRule);
}
