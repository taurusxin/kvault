const UTILITY_WIDTH = 52 + 72;
const KEY_MIN_WIDTH = 112;
const VALUE_MIN_WIDTH = 80;

export function fitRecordColumns(
  keyPreference: number,
  valuePreference: number,
  viewportWidth: number,
) {
  const available = Math.max(0, viewportWidth - UTILITY_WIDTH);
  const scale = Math.min(1, available / (KEY_MIN_WIDTH + VALUE_MIN_WIDTH));
  const minKey = KEY_MIN_WIDTH * scale;
  const minValue = VALUE_MIN_WIDTH * scale;
  const key = Math.min(
    Math.max(minKey, keyPreference),
    Math.max(0, available - minValue),
  );
  const value = Math.min(
    Math.max(minValue, valuePreference),
    Math.max(0, available - key),
  );
  return {
    key: Number.isFinite(available) ? available - value : key,
    value,
    minKey,
    minValue,
  };
}

export type RecordColumnWidths = ReturnType<typeof fitRecordColumns>;
