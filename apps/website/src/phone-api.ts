// SPDX-License-Identifier: GPL-3.0-or-later

/**
 * Phone-only fixtures shared by the website Live Demo (`mobile-demo.tsx`) and
 * the README capture mode (`apps/mobile/src/capture-api.ts`).
 *
 * The browser mock in `browser-api.ts` is desktop-shaped, so the phone shell
 * layers the Android state on top: an already granted VPN permission, a tunnel
 * that follows the mocked core, the installed-app list, per-app routing, and
 * the state-changed announcements a real backend emits after mutations.
 */
import type {
  ApiContract,
  AppProxyMode,
  AppSettings,
  InstalledApp,
  StatusResponse,
} from "../../desktop/src/api/contracts";
import { readLanguagePreference } from "../../desktop/src/lib/language";
import chromeIcon from "./assets/app-icons/chrome.svg?url";
import discordIcon from "./assets/app-icons/discord.svg?url";
import githubIcon from "./assets/app-icons/github.svg?url";
import gmailIcon from "./assets/app-icons/gmail.svg?url";
import googleCalendarIcon from "./assets/app-icons/google-calendar.svg?url";
import googleDriveIcon from "./assets/app-icons/google-drive.svg?url";
import googleMapsIcon from "./assets/app-icons/google-maps.svg?url";
import googlePhotosIcon from "./assets/app-icons/google-photos.svg?url";
import instagramIcon from "./assets/app-icons/instagram.svg?url";
import netflixIcon from "./assets/app-icons/netflix.svg?url";
import redditIcon from "./assets/app-icons/reddit.svg?url";
import signalIcon from "./assets/app-icons/signal.svg?url";
import slackIcon from "./assets/app-icons/slack.svg?url";
import spotifyIcon from "./assets/app-icons/spotify.svg?url";
import telegramIcon from "./assets/app-icons/telegram.svg?url";
import tiktokIcon from "./assets/app-icons/tiktok.svg?url";
import twitchIcon from "./assets/app-icons/twitch.svg?url";
import whatsappIcon from "./assets/app-icons/whatsapp.svg?url";
import xIcon from "./assets/app-icons/x.svg?url";
import youtubeIcon from "./assets/app-icons/youtube.svg?url";

/** A realistic Android app list with launcher icons; every routing mode has
 * at least one row, and the rest stay on the default follow rules. */
const apps: InstalledApp[] = [
  { package_name: "com.google.android.youtube", label: "YouTube", system: false, icon: youtubeIcon },
  { package_name: "com.android.chrome", label: "Chrome", system: true, icon: chromeIcon },
  { package_name: "org.telegram.messenger", label: "Telegram", system: false, icon: telegramIcon },
  { package_name: "com.spotify.music", label: "Spotify", system: false, icon: spotifyIcon },
  { package_name: "com.google.android.gm", label: "Gmail", system: true, icon: gmailIcon },
  { package_name: "com.github.android", label: "GitHub", system: false, icon: githubIcon },
  { package_name: "com.netflix.mediaclient", label: "Netflix", system: false, icon: netflixIcon },
  { package_name: "com.google.android.apps.maps", label: "Maps", system: true, icon: googleMapsIcon },
  { package_name: "com.instagram.android", label: "Instagram", system: false, icon: instagramIcon },
  { package_name: "com.whatsapp", label: "WhatsApp", system: false, icon: whatsappIcon },
  { package_name: "com.discord", label: "Discord", system: false, icon: discordIcon },
  { package_name: "com.Slack", label: "Slack", system: false, icon: slackIcon },
  { package_name: "com.zhiliaoapp.musically", label: "TikTok", system: false, icon: tiktokIcon },
  { package_name: "com.twitter.android", label: "X", system: false, icon: xIcon },
  { package_name: "org.thoughtcrime.securesms", label: "Signal", system: false, icon: signalIcon },
  { package_name: "com.reddit.frontpage", label: "Reddit", system: false, icon: redditIcon },
  { package_name: "tv.twitch.android.app", label: "Twitch", system: false, icon: twitchIcon },
  { package_name: "com.google.android.apps.photos", label: "Photos", system: true, icon: googlePhotosIcon },
  { package_name: "com.google.android.apps.docs", label: "Drive", system: true, icon: googleDriveIcon },
  { package_name: "com.google.android.calendar", label: "Calendar", system: true, icon: googleCalendarIcon },
];

const appProxies: Record<string, AppProxyMode> = {
  "com.google.android.youtube": "proxy",
  "com.android.chrome": "proxy",
  "org.telegram.messenger": "proxy",
  "com.google.android.gm": "direct",
  "com.github.android": "block",
  "com.netflix.mediaclient": "proxy",
  "com.instagram.android": "proxy",
  "com.whatsapp": "direct",
  "com.discord": "proxy",
  "com.Slack": "block",
  "com.zhiliaoapp.musically": "block",
  "com.twitter.android": "proxy",
  "org.thoughtcrime.securesms": "direct",
  "com.reddit.frontpage": "proxy",
  "com.google.android.apps.docs": "direct",
};

/**
 * Apply the phone fixtures to an API object in place. The website demo patches
 * the shared `browser-api` singleton, which `App` imports through the
 * `@platform/api` alias, so the overrides must land on that same object.
 */
export function applyPhoneFixtures(api: ApiContract): void {
  const proxies = { ...appProxies };
  const stateChangedHandlers = new Set<() => void>();

  function announceStateChanged() {
    for (const handler of stateChangedHandlers) handler();
  }

  const browserGetStatus = api.getStatus.bind(api);
  const browserGetSettings = api.getSettings.bind(api);
  const browserStart = api.start.bind(api);
  const browserStop = api.stop.bind(api);

  api.getStatus = async (): Promise<StatusResponse> => {
    const status = await browserGetStatus();
    return {
      ...status,
      // The phone power button follows the tunnel, not the desktop core state.
      vpn_permission: "granted",
      tunnel_status: status.core.status === "running" ? "connected" : "stopped",
    };
  };

  api.getSettings = async (): Promise<AppSettings> => {
    // The mock pins the English language; follow the visitor's pick instead, the
    // same way the desktop demo does. Return a copy so callers cannot mutate the
    // fixture map behind `setAppProxy`.
    return {
      ...(await browserGetSettings()),
      language: readLanguagePreference(),
      app_proxies: { ...proxies },
    };
  };

  api.listApps = async (): Promise<InstalledApp[]> =>
    apps.map((app) => ({ ...app }));

  api.setAppProxy = async (
    packageName: string,
    mode: AppProxyMode,
  ): Promise<void> => {
    if (mode === "follow") delete proxies[packageName];
    else proxies[packageName] = mode;
  };

  // The browser mock has no backend events, so synthesize the announcements
  // after the actions that change what the node list and Home read.
  api.listenStateChanged = async (
    handler: () => void,
  ): Promise<() => void> => {
    stateChangedHandlers.add(handler);
    return () => {
      stateChangedHandlers.delete(handler);
    };
  };

  api.start = async (): Promise<void> => {
    await browserStart();
    announceStateChanged();
  };

  api.stop = async (): Promise<void> => {
    await browserStop();
    announceStateChanged();
  };
}

/** Phone fixtures on a copy of `base`, for the README capture API. */
export function createPhoneApi(base: ApiContract): ApiContract {
  const api: ApiContract = { ...base };
  applyPhoneFixtures(api);
  return api;
}
