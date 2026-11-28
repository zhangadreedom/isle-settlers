// Progressive Web App registration and desktop/home-screen install prompt.
// Disabled completely in `pnpm build:embed` sandboxed artifact builds.

import { createSignal } from 'solid-js';
import { EMBEDDED } from './embed';

export interface BeforeInstallPromptEvent extends Event {
  readonly platforms?: string[];
  readonly userChoice: Promise<{ outcome: 'accepted' | 'dismissed'; platform: string }>;
  prompt(): Promise<void>;
}

const [installable, setInstallable] = createSignal(false);
let deferredPrompt: BeforeInstallPromptEvent | null = null;
let initialized = false;

/** Reactive Solid signal indicating whether the browser offers `"安装到桌面"`. */
export const canInstallPwa = installable;

function isStandalone(): boolean {
  if (typeof window === 'undefined') return false;
  return (
    window.matchMedia?.('(display-mode: standalone)').matches === true ||
    (navigator as Navigator & { standalone?: boolean }).standalone === true
  );
}

function collectLoadedAssetUrls(): string[] {
  if (typeof performance === 'undefined' || typeof performance.getEntriesByType !== 'function') {
    return [];
  }
  const urls = new Set<string>();
  for (const entry of performance.getEntriesByType('resource')) {
    try {
      const url = new URL(entry.name, location.origin);
      if (
        url.origin === location.origin &&
        url.pathname.includes('/assets/') &&
        !url.search.includes('debug')
      ) {
        urls.add(url.href);
      }
    } catch {
      // Ignore non-URL resource entries.
    }
  }
  return [...urls];
}

async function warmAssetsInServiceWorker(): Promise<void> {
  try {
    const reg = await navigator.serviceWorker.ready;
    const urls = collectLoadedAssetUrls();
    if (urls.length > 0 && reg.active) {
      reg.active.postMessage({ type: 'WARM_ASSETS', urls });
    }
  } catch {
    // Offline warm-up is best-effort.
  }
}

async function registerServiceWorker(): Promise<void> {
  try {
    await navigator.serviceWorker.register('./sw.js', { scope: './' });
    await warmAssetsInServiceWorker();
    // Catch lazy-loaded Three.js & WASM worker chunks fetched right after boot.
    setTimeout(() => {
      void warmAssetsInServiceWorker();
    }, 4000);
  } catch {
    // Blocked in private browsing or restricted contexts; app still works online.
  }
}

/**
 * Initializes PWA manifest link, install prompt listeners, and Service Worker.
 * Strict no-op in `EMBEDDED` builds and on non-HTTP(S) protocols; skips SW
 * registration when `?debug` is present in the URL.
 */
export function initPwa(): void {
  if (EMBEDDED || initialized || typeof window === 'undefined') return;
  if (location.protocol !== 'http:' && location.protocol !== 'https:') return;
  initialized = true;

  if (!document.querySelector('link[rel="manifest"]')) {
    const link = document.createElement('link');
    link.rel = 'manifest';
    link.href = './manifest.webmanifest';
    document.head.appendChild(link);
  }

  if (!isStandalone()) {
    window.addEventListener('beforeinstallprompt', (event) => {
      event.preventDefault();
      deferredPrompt = event as BeforeInstallPromptEvent;
      setInstallable(true);
    });
  }

  window.addEventListener('appinstalled', () => {
    deferredPrompt = null;
    setInstallable(false);
  });

  window.matchMedia?.('(display-mode: standalone)').addEventListener?.('change', (event) => {
    if (event.matches) {
      deferredPrompt = null;
      setInstallable(false);
    }
  });

  if (location.search.includes('debug')) return;
  if (!('serviceWorker' in navigator)) return;

  void registerServiceWorker();
}

/**
 * Triggers the native PWA install dialog if available.
 * Resolves to `true` when the user accepts installation, `false` otherwise.
 */
export async function promptInstallPwa(): Promise<boolean> {
  const promptEvent = deferredPrompt;
  if (!promptEvent) return false;

  deferredPrompt = null;
  setInstallable(false);

  try {
    await promptEvent.prompt();
    const choice = await promptEvent.userChoice;
    return choice.outcome === 'accepted';
  } catch {
    return false;
  }
}
