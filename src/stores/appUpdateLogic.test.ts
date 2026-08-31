import { describe, expect, it } from "vitest";
import {
  APP_UPDATE_AUTO_OPEN_DELAY_MS,
  shouldAutoOpenForSession,
  shouldAutoOpenModal,
  shouldOpenModalFromManualCheck,
  shouldShowPassivePrompt,
} from "./appUpdateLogic";
import type { AppUpdateInfo } from "../types";

function info(partial: Partial<AppUpdateInfo>): AppUpdateInfo {
  return {
    checked: true,
    updateAvailable: false,
    promptSuppressed: false,
    ignored: false,
    currentVersion: "1.5.2",
    latestVersion: "1.5.3",
    notes: "",
    giteePage: "",
    githubPage: "",
    setupUrl: "",
    source: "gitee",
    ...partial,
  };
}

describe("shouldShowPassivePrompt", () => {
  it("shows when newer and not suppressed", () => {
    expect(shouldShowPassivePrompt(info({ updateAvailable: true, promptSuppressed: false }))).toBe(
      true,
    );
  });

  it("hides when newer but suppressed", () => {
    expect(
      shouldShowPassivePrompt(
        info({ updateAvailable: true, promptSuppressed: true, ignored: true }),
      ),
    ).toBe(false);
  });

  it("hides when no update", () => {
    expect(shouldShowPassivePrompt(info({ updateAvailable: false }))).toBe(false);
  });
});

describe("shouldOpenModalFromManualCheck", () => {
  it("opens when newer even if suppressed", () => {
    expect(
      shouldOpenModalFromManualCheck(
        info({ updateAvailable: true, promptSuppressed: true, ignored: true }),
      ),
    ).toBe(true);
  });

  it("does not open when no update", () => {
    expect(shouldOpenModalFromManualCheck(info({ updateAvailable: false }))).toBe(false);
  });
});

describe("shouldAutoOpenModal", () => {
  it("matches passive prompt rules", () => {
    expect(shouldAutoOpenModal(info({ updateAvailable: true, promptSuppressed: false }))).toBe(
      true,
    );
    expect(shouldAutoOpenModal(info({ updateAvailable: true, promptSuppressed: true }))).toBe(
      false,
    );
  });
});

describe("shouldAutoOpenForSession", () => {
  it("allows when not yet dismissed this session", () => {
    expect(shouldAutoOpenForSession("1.6.0", null)).toBe(true);
    expect(shouldAutoOpenForSession("1.6.0", "1.5.9")).toBe(true);
  });

  it("blocks when this version was closed or ignored in-session", () => {
    expect(shouldAutoOpenForSession("1.6.0", "1.6.0")).toBe(false);
  });

  it("blocks empty latest", () => {
    expect(shouldAutoOpenForSession("", null)).toBe(false);
    expect(shouldAutoOpenForSession(null, null)).toBe(false);
  });
});

describe("APP_UPDATE_AUTO_OPEN_DELAY_MS", () => {
  it("defers long enough for bridge init (at least 8s)", () => {
    expect(APP_UPDATE_AUTO_OPEN_DELAY_MS).toBeGreaterThanOrEqual(8_000);
  });
});
