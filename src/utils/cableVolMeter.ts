/** 虚拟声卡送声标尺：输出电平 dBFS（已含 live 增益） */

export const CABLE_VOL_DB_MIN = -42;
export const CABLE_VOL_DB_MAX = 0;
export const CABLE_VOL_DB_LOW = -20;
export const CABLE_VOL_DB_HIGH = -4;

export const CABLE_VOL_TICKS = [-36, -24, -12, -6, 0] as const;

export function cableLevelToDb(level: number): number {
  const v = Math.max(0, Math.min(1, level));
  if (v < 0.00025) return CABLE_VOL_DB_MIN;
  return Math.max(
    CABLE_VOL_DB_MIN,
    Math.min(CABLE_VOL_DB_MAX, 20 * Math.log10(v))
  );
}

export function cableDbToPct(db: number): number {
  return (
    ((db - CABLE_VOL_DB_MIN) / (CABLE_VOL_DB_MAX - CABLE_VOL_DB_MIN)) * 100
  );
}

export type CableVolZone = "low" | "ok" | "high" | "idle";

export function cableZoneForDb(db: number): CableVolZone {
  if (db <= CABLE_VOL_DB_MIN + 0.5) return "idle";
  if (db < CABLE_VOL_DB_LOW) return "low";
  if (db > CABLE_VOL_DB_HIGH) return "high";
  return "ok";
}

export function cableZoneForLevel(level: number): CableVolZone {
  return cableZoneForDb(cableLevelToDb(level));
}
