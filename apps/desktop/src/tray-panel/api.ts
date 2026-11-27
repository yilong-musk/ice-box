// SPDX-License-Identifier: GPL-3.0-or-later

import { invoke } from "@tauri-apps/api/core";

export interface PanelActionRow {
  id: string;
  label: string;
  enabled: boolean;
}

export interface PanelSubmenuHeader {
  label: string;
  enabled: boolean;
}

export interface PanelCheckRow {
  id: string;
  label: string;
  enabled: boolean;
  checked: boolean;
}

export interface PanelDelayButton {
  id: string;
  label: string;
  enabled: boolean;
}

export interface PanelDelayBadge {
  text: string;
  tone: "ok" | "warn" | "bad" | null;
}

export interface PanelNodeMember {
  id: string;
  label: string;
  enabled: boolean;
  checked: boolean;
  delay: PanelDelayBadge | null;
}

export type PanelNodeEntry =
  | {
      kind: "item";
      id: string;
      label: string;
      enabled: boolean;
      checked: boolean;
      delay: PanelDelayBadge | null;
    }
  | {
      kind: "group";
      tag: string;
      label: string;
      delay: PanelDelayBadge | null;
      delayButton: PanelDelayButton;
      members: PanelNodeMember[];
    };

export interface PanelModel {
  update: PanelActionRow | null;
  service: PanelActionRow;
  mode: PanelSubmenuHeader;
  modeItems: PanelCheckRow[];
  nodes: PanelSubmenuHeader;
  nodesDelay: PanelDelayButton | null;
  nodeEntries: PanelNodeEntry[];
  subs: PanelSubmenuHeader;
  subItems: PanelCheckRow[];
  copyCliProxy: PanelActionRow;
  openCliProxy: PanelActionRow;
  mcp: PanelActionRow;
  copyMcpConfig: PanelActionRow;
  show: PanelActionRow;
  quit: PanelActionRow;
}

export interface PanelPlacement {
  seq: number;
  scaleFactor: number;
  side: "left" | "right";
  edge: "top" | "bottom";
  boundsTop: number;
  boundsBottom: number;
  mainWidth: number;
  modeWidth: number;
  subsWidth: number;
  subWidth: number;
  leafWidth: number;
  gap: number;
  radius: number;
}

export interface CardRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface PanelLayout {
  seq: number;
  width: number;
  height: number;
  offsetX: number;
  offsetY: number;
  cards: CardRect[];
  dark: boolean | null;
}

export const SHOW_EVENT = "tray-panel://show";
export const HIDE_EVENT = "tray-panel://hide";
export const CHANGED_EVENT = "tray-panel://changed";

export const trayPanelApi = {
  state: () => invoke<PanelModel>("tray_panel_state"),
  placement: () => invoke<PanelPlacement | null>("tray_panel_placement"),
  layout: (layout: PanelLayout) => invoke<void>("tray_panel_layout", { layout }),
  hide: () => invoke<void>("tray_panel_hide"),
  action: (id: string) => invoke<void>("tray_panel_action", { id }),
};
