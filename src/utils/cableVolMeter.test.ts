import { describe, expect, it } from "vitest";
import {
  cableDbToPct,
  cableLevelToDb,
  cableZoneForDb,
  cableZoneForLevel,
} from "./cableVolMeter";

describe("cableVolMeter", () => {
  it("maps silence to floor dB", () => {
    expect(cableLevelToDb(0)).toBe(-42);
  });

  it("maps full scale to 0 dBFS", () => {
    expect(cableLevelToDb(1)).toBe(0);
  });

  it("classifies zones on dB thresholds", () => {
    expect(cableZoneForDb(-42)).toBe("idle");
    expect(cableZoneForDb(-24)).toBe("low");
    expect(cableZoneForDb(-12)).toBe("ok");
    expect(cableZoneForDb(-2)).toBe("high");
  });

  it("converts level to pct monotonically", () => {
    expect(cableDbToPct(-42)).toBe(0);
    expect(cableDbToPct(0)).toBe(100);
    expect(cableZoneForLevel(0.5)).toBe("ok");
  });
});
