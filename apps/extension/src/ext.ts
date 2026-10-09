// Firefox exposes promise-based APIs as `browser`; Chromium as `chrome`.
export const ext: typeof chrome = (globalThis as { browser?: typeof chrome }).browser ?? chrome;
