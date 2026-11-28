// SPDX-License-Identifier: GPL-3.0-or-later

import {
  CLI_INSTALL_COMMAND,
  CLI_TARGETS,
  cliDownloadUrl,
  defaultCliShell,
} from "./downloads";
import { marketingText } from "./i18n";
import "./cli-download.css";

const CLI_SHELL_OPTIONS = [
  ["unix", "macOS / Linux"],
  ["windows", "Windows"],
];

const CLI_ARCHIVE_LABELS = {
  "darwin-aarch64": "macOS (arm64)",
  "windows-x86_64": "Windows (x64)",
  "linux-x86_64": "Linux (x64)",
  "linux-aarch64": "Linux (arm64)",
};

/** Markup for the headless CLI card under the desktop download panel. */
export function renderCliDownloadCard({
  icon,
  t,
  displayVersion,
  appVersion,
  repoUrl,
  platform,
}) {
  const selectedCliShell = defaultCliShell(platform);
  return `<div class="download-cli-card reveal" data-cli-card>
        <div class="download-cli-head">
          <span class="download-emblem download-cli-emblem">${icon("terminal", "platform-mark")}</span>
          <div class="download-pane-identity">
            <div class="download-pane-title-row">
              <h3 class="download-cli-title">ice-box CLI</h3>
              <span class="download-version">${displayVersion}</span>
            </div>
            <p class="download-cli-subtitle" ${t("download.cli.subtitle")}></p>
          </div>
          <div class="download-arch-group" role="group" data-cli-shell-group>
            ${CLI_SHELL_OPTIONS.map(
              ([shell, label]) =>
                `<button type="button" class="download-arch-option" data-cli-shell="${shell}" aria-pressed="${shell === selectedCliShell ? "true" : "false"}">${label}</button>`,
            ).join("")}
          </div>
        </div>
        <div class="download-cli-command" dir="ltr">
          <pre class="download-cli-code"><code data-cli-command>${CLI_INSTALL_COMMAND[selectedCliShell]}</code></pre>
          <button type="button" class="button download-cli-copy" data-cli-copy>
            <span class="download-cli-copy-icon">${icon("copy", "icon icon-xs")}</span>
            <span class="download-cli-copied-icon" hidden>${icon("check", "icon icon-xs")}</span>
            <span class="download-cli-copy-label"></span>
          </button>
        </div>
        <div class="download-cli-footer">
          <div class="download-cli-archives">
            <span class="download-cli-archives-label" ${t("download.cli.archives")}></span>
            <div class="download-cli-links">
              ${CLI_TARGETS.map(
                (target) =>
                  `<a class="download-cli-link" data-cli-archive="${target}" href="${cliDownloadUrl(appVersion, target)}">${icon("download", "icon icon-xs")}<span>${CLI_ARCHIVE_LABELS[target]}</span></a>`,
              ).join("")}
            </div>
          </div>
          <a class="text-link download-cli-docs" href="${repoUrl}/blob/main/docs/cli.md" target="_blank" rel="noreferrer"><span ${t("download.cli.docs")}></span>${icon("arrowRight", "icon icon-xs")}</a>
        </div>
      </div>`;
}

/**
 * Wire shell switching and the copy button.
 * `setLanguage` refreshes the group label and the copy button.
 */
export function bindCliDownloadCard(card) {
  const shellGroup = card.querySelector("[data-cli-shell-group]");
  const command = card.querySelector("[data-cli-command]");
  const copyButton = card.querySelector("[data-cli-copy]");
  const copyLabel = card.querySelector(".download-cli-copy-label");
  const copyIcon = card.querySelector(".download-cli-copy-icon");
  const copiedIcon = card.querySelector(".download-cli-copied-icon");

  let selectedCliShell =
    shellGroup.querySelector('[aria-pressed="true"]')?.dataset.cliShell === "windows"
      ? "windows"
      : "unix";
  let cliCopied = false;
  let cliCopyTimer = null;
  let language = null;

  function syncCopyButton() {
    if (language == null) return;
    const label = marketingText(
      cliCopied ? "download.cli.copied" : "download.cli.copy",
      language,
    );
    copyLabel.textContent = label;
    copyButton.setAttribute("aria-label", label);
    copyButton.classList.toggle("is-copied", cliCopied);
    copyIcon.hidden = cliCopied;
    copiedIcon.hidden = !cliCopied;
  }

  function selectCliShell(shell) {
    if (shell !== "unix" && shell !== "windows") return;
    selectedCliShell = shell;
    if (cliCopyTimer != null) {
      window.clearTimeout(cliCopyTimer);
      cliCopyTimer = null;
    }
    cliCopied = false;
    for (const option of shellGroup.querySelectorAll("[data-cli-shell]")) {
      option.setAttribute(
        "aria-pressed",
        option.dataset.cliShell === selectedCliShell ? "true" : "false",
      );
    }
    command.textContent = CLI_INSTALL_COMMAND[selectedCliShell];
    syncCopyButton();
  }

  async function copyCliCommand() {
    const text = CLI_INSTALL_COMMAND[selectedCliShell];
    let copied = false;
    if (navigator.clipboard?.writeText) {
      try {
        await navigator.clipboard.writeText(text);
        copied = true;
      } catch {
        // Fall back to a hidden textarea when clipboard permissions are unavailable.
      }
    }
    if (!copied) {
      const area = document.createElement("textarea");
      area.value = text;
      area.setAttribute("readonly", "");
      area.style.position = "fixed";
      area.style.opacity = "0";
      document.body.appendChild(area);
      area.select();
      try {
        copied = document.execCommand("copy");
      } catch {
        copied = false;
      }
      area.remove();
    }
    if (!copied) return;
    cliCopied = true;
    syncCopyButton();
    if (cliCopyTimer != null) window.clearTimeout(cliCopyTimer);
    cliCopyTimer = window.setTimeout(() => {
      cliCopied = false;
      cliCopyTimer = null;
      syncCopyButton();
    }, 2000);
  }

  shellGroup.addEventListener("click", (event) => {
    const button = event.target.closest("[data-cli-shell]");
    if (!button) return;
    selectCliShell(button.dataset.cliShell);
  });

  copyButton.addEventListener("click", () => {
    void copyCliCommand();
  });

  return {
    setLanguage(next) {
      language = next;
      shellGroup.setAttribute(
        "aria-label",
        marketingText("download.cli.shellAria", next),
      );
      syncCopyButton();
    },
  };
}
