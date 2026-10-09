// Toolbar popup: connection status and capture settings.

import { ext } from "./ext";
import { normalizeSettings, parseExtensions, type Settings } from "./policy";
import { HOST_NAME, type HostReply } from "./protocol";
import { describe } from "./status";

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const status = $<HTMLParagraphElement>("status");
const enabled = $<HTMLInputElement>("enabled");
const extensions = $<HTMLTextAreaElement>("extensions");
const minSize = $<HTMLInputElement>("min-size");

async function load(): Promise<Settings> {
  return normalizeSettings((await ext.storage.local.get("settings")).settings as Partial<Settings> | undefined);
}

async function save(patch: Partial<Settings>) {
  await ext.storage.local.set({ settings: { ...(await load()), ...patch } });
}

async function init() {
  const settings = await load();
  enabled.checked = settings.enabled;
  extensions.value = settings.extensions.join(" ");
  minSize.value = String(Math.round(settings.minSizeBytes / 1024));

  enabled.addEventListener("change", () => void save({ enabled: enabled.checked }));
  $("save").addEventListener("click", async () => {
    const list = parseExtensions(extensions.value);
    await save({ extensions: list, minSizeBytes: Math.max(0, Number(minSize.value) || 0) * 1024 });
    extensions.value = list.join(" ");
    const saved = $("saved");
    saved.hidden = false;
    setTimeout(() => (saved.hidden = true), 1500);
  });

  const reply = await (ext.runtime.sendNativeMessage(HOST_NAME, { type: "ping" }) as Promise<HostReply>).catch(
    (e: unknown) => (e instanceof Error ? e : new Error(String(e))),
  );
  const { state, text } = describe(reply);
  status.dataset.state = state;
  status.textContent = text;
  // A successful ping means the app works again; clear an old error badge.
  if (state !== "missing") void ext.action.setBadgeText({ text: "" });
}

void init();
