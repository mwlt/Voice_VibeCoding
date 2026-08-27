import type { AppUpdateInfo } from "../types";

/** 顶栏角标、启动自动弹窗：有新版本且未忽略自动提醒 */
export function shouldShowPassivePrompt(info: AppUpdateInfo | null | undefined): boolean {
  return Boolean(info?.updateAvailable && !info?.promptSuppressed);
}

/** 被动检测是否应自动打开弹窗（另需 session 未 dismiss） */
export function shouldAutoOpenModal(info: AppUpdateInfo | null | undefined): boolean {
  return shouldShowPassivePrompt(info);
}

/** 设置页主动检查：只要有新版本就应弹窗（含已忽略） */
export function shouldOpenModalFromManualCheck(info: AppUpdateInfo | null | undefined): boolean {
  return Boolean(info?.updateAvailable);
}
