// 独立设置窗口工具(主窗口与聊天页共用)

import { WebviewWindow } from "@tauri-apps/api/webviewWindow";

/** 打开(或聚焦)独立设置窗口 */
export async function openSettingsWindow() {
  const label = "settings";
  // 若已存在则聚焦
  const existing = await WebviewWindow.getByLabel(label);
  if (existing) {
    await existing.setFocus();
    return;
  }
  const win = new WebviewWindow(label, {
    url: "/settings",
    title: "参数设置",
    width: 560,
    height: 640,
    resizable: true,
    minimizable: false,
    maximizable: false,
    center: true,
  });
  win.once("tauri://created", async () => {
    try {
      await win.center();
    } catch {
      // ignore
    }
  });
}
