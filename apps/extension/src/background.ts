// Background script: wires the browser's APIs to the capture logic.

import { cookieHeader, createCapture } from "./capture";
import { ext } from "./ext";
import { normalizeSettings, type Settings } from "./policy";
import { HOST_NAME, type HostMessage, type HostReply } from "./protocol";

const MENU_ID = "nexusdl-download-link";

const capture = createCapture({
  settings: async () => normalizeSettings((await ext.storage.local.get("settings")).settings as Partial<Settings> | undefined),
  cancel: async (id) => {
    await ext.downloads.cancel(id);
    await ext.downloads.erase({ id });
  },
  cookies: async (url) => cookieHeader(await ext.cookies.getAll({ url })),
  sendToHost: (message: HostMessage) => ext.runtime.sendNativeMessage(HOST_NAME, message) as Promise<HostReply>,
  download: async (url) => {
    await ext.downloads.download({ url });
  },
  report: (error) => {
    void ext.action.setBadgeText({ text: error ? "!" : "" });
    void ext.action.setBadgeBackgroundColor({ color: "#e5484d" });
    void ext.action.setTitle({ title: error ? `NexusDL: ${error}` : "NexusDL" });
  },
  userAgent: navigator.userAgent,
});

ext.downloads.onCreated.addListener((item) => void capture.onCreated(item));

ext.runtime.onInstalled.addListener(() => {
  ext.contextMenus.create({
    id: MENU_ID,
    title: "Download with NexusDL",
    contexts: ["link", "video", "audio"],
  });
});

ext.contextMenus.onClicked.addListener((info) => {
  if (info.menuItemId !== MENU_ID) return;
  const url = info.linkUrl ?? info.srcUrl;
  if (url) void capture.sendLink(url, info.pageUrl);
});
