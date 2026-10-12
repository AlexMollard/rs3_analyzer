// Typed wrappers over the Tauri commands. In `pnpm dev` with ?mock in the URL, a fake backend
// is used so the UI can run in a plain browser; the mock is never part of a production build.
import { Channel, invoke } from '@tauri-apps/api/core';
import type { Backend, FavoritesSnapshot, HistoryPoint, ScanResult } from './types.ts';

const tauriBackend: Backend = {
  scan(onProgress) {
    const channel = new Channel<number>(onProgress);
    return invoke<ScanResult>('scan', { onProgress: channel });
  },
  itemHistory: (itemId) => invoke<HistoryPoint[]>('item_history', { itemId }),
  getFavorites: () => invoke<FavoritesSnapshot>('get_favorites'),
  setFavorite: (name, on) => invoke<string[]>('set_favorite', { name, on }),
};

let backendPromise: Promise<Backend> | undefined;

export function backend(): Promise<Backend> {
  if (!backendPromise) {
    backendPromise =
      import.meta.env.DEV && new URLSearchParams(location.search).has('mock')
        ? import('./mock.ts').then((m) => m.mockBackend)
        : Promise.resolve(tauriBackend);
  }
  return backendPromise;
}

/** Commands reject with a string, or with { message, searched } for scan. */
export function errorText(e: unknown): string {
  if (typeof e === 'string') return e;
  if (e && typeof e === 'object' && 'message' in e) return String((e as { message: unknown }).message);
  return String(e);
}
