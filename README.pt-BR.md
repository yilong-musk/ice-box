<div align="center">

<a href="https://yilong-musk.github.io/ice-box/">
  <img src="docs/brand/switch-logo/assets/logo.gif" alt="Logo do ice-box" width="144">
</a>

# ice-box

**A melhor sing-box GUI.**

Cliente de proxy leve para macOS, Windows, Linux e Android — construído sobre o [sing-box](https://github.com/SagerNet/sing-box).

[![Release](https://img.shields.io/github/v/release/yilong-musk/ice-box?style=flat-square&label=release)](https://github.com/yilong-musk/ice-box/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/yilong-musk/ice-box/ci.yml?branch=main&style=flat-square&label=CI)](https://github.com/yilong-musk/ice-box/actions/workflows/ci.yml)
[![Downloads](https://img.shields.io/github/downloads/yilong-musk/ice-box/total?style=flat-square)](https://github.com/yilong-musk/ice-box/releases)
[![Platforms](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux%20%7C%20Android-007ec6?style=flat-square)](#instalação)
[![Core](https://img.shields.io/badge/sing--box-1.14.4-6f42c1?style=flat-square)](https://github.com/SagerNet/sing-box)
[![License](https://img.shields.io/github/license/yilong-musk/ice-box?style=flat-square)](LICENSE)

[**Download**](https://github.com/yilong-musk/ice-box/releases/latest) · [**Demo ao vivo**](https://yilong-musk.github.io/ice-box/) · [Instalação](#instalação) · [Desenvolvimento](#desenvolvimento) · [Documentação](#documentação) · [Changelog](CHANGELOG.md)

[Bahasa Indonesia](README.id.md) · [Deutsch](README.de.md) · [English](README.md) · [Español](README.es.md) · [Français](README.fr.md) · **Português (BR)** · [Tiếng Việt](README.vi.md) · [Türkçe](README.tr.md)<br>[Русский](README.ru.md) · [Українська](README.uk.md) · [العربية](README.ar.md) · [فارسی](README.fa.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [简体中文](README.zh-CN.md) · [繁體中文](README.zh-TW.md)

<br>

<a href="https://yilong-musk.github.io/ice-box/">
  <img src="docs/images/hero.png" alt="Interface desktop e Android do ice-box" width="880">
</a>

<sub>Experimente as interfaces desktop e Android no navegador pela <a href="https://yilong-musk.github.io/ice-box/">Demo ao vivo</a>.</sub>

</div>

## Recursos

- **Assinaturas**: Detecta automaticamente sing-box JSON, Clash, links de compartilhamento e arquivos JSON locais; exibe cota/validade; atualização automática no desktop.
- **Roteamento e nós**: Modos **Regra** / **Global** / **Direto**, interruptores por regra, regras personalizadas, testes de latência e favoritos.
- **Captura de tráfego**: Proxy do sistema ou TUN no desktop ([`docs/tun.md`](docs/tun.md)); VPN do sistema com roteamento por app no Android.
- **Conexões e bandeja (Desktop)**: Inspetor de conexões em tempo real com busca/ordenação/encerramento, além de controles na bandeja (velocidade e testes de latência na barra de menus do macOS).
- **MCP para Agentes (Desktop)**: Servidor MCP local opcional autenticado em loopback com ponte stdio integrada compartilhando a sessão ativa ([`docs/mcp.md`](docs/mcp.md)).
- **Essenciais**: Iniciar no login, atualizações assinadas no app, cartões de tráfego e memória em tempo real, temas claro/escuro e 16 idiomas com suporte completo a RTL (`ar`, `fa`).

## Instalação

Baixe em [GitHub Releases](https://github.com/yilong-musk/ice-box/releases/latest):

| Plataforma | Requisitos | Pacote | Observações |
|---|---|---|---|
| **macOS** (Apple Silicon) | macOS 12+ | `ice-box_<version>_aarch64.dmg` | Assinado de forma ad-hoc: permita em **Privacidade e Segurança → Abrir Mesmo Assim**, ou execute `xattr -dr com.apple.quarantine /Applications/ice-box.app`. |
| **Windows** (x64) | Windows 10+ | `ice-box_<version>_x64-setup.exe` | Instalador NSIS por usuário (sem assinatura; confirme no SmartScreen). O TUN suporta apenas IPv4 TCP e exige Administrador. |
| **Linux** (x64 / arm64) | Ubuntu 22.04+ | `.AppImage` / `.deb` | `chmod +x *.AppImage` ou `sudo apt install ./ice-box_*.deb`. A bandeja do GNOME requer `gnome-shell-extension-appindicator`. |
| **Android** (arm64) | Android 14+ | `ice-box_<version>_android_arm64.apk` | Assinado com o keystore do projeto; permita instalar desta fonte. |

- **Início rápido**: Importe uma assinatura (**Assinaturas**) → escolha modo e nó (**Início** / **Nós**) → pressione energia. Funciona em modo direto sem assinatura.
- **Atualizações**: Atualizações assinadas no app em **Ajustes → Atualizações do app** (`.deb` abre Releases para o `apt`; o Android baixa o APK para instalação pelo sistema).

## Desenvolvimento

Requer Rust (`rust-toolchain.toml`), Node.js 22, Xcode CLT (macOS) ou bibliotecas de desenvolvimento GTK/WebKit (`libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`, `libssl-dev`) no Linux. O Android adiciona JDK 17, Go e `ANDROID_HOME` (`scripts/ci-install-android-sdk.sh`).

```bash
cd apps/desktop && npm install && cd ../..
npm run fetch-singbox
npm run dev
```

| Comando | Finalidade |
|---|---|
| `npm run dev:mac-arm64` / `dev:mac-x64` / `dev:win` / `dev:linux-x64` / `dev:linux-arm64` | Executar para um alvo de host específico |
| `npm run gate` | Gate completo do workspace (fmt, clippy, testes Rust/frontend, builds, smoke tests) |
| `npm run fetch-singbox && npm run build` | Gerar `.dmg` para macOS |
| `npm run fetch-singbox -- win && npm run build:win` | Gerar instalador para Windows |
| `npm run fetch-singbox -- linux-x64 && npm run build:linux` | Gerar `.deb` e `.AppImage` para Linux (`linux-arm64` em arm64) |
| `npm run acceptance` / `acceptance:tun` / `acceptance:win` / `acceptance:linux` / `acceptance:linux-tun` | Gates de aceitação em host real |
| `bash scripts/build-android-apk.sh debug arm64` | Gerar APK de depuração Android (`amd64` para emulador) |

```text
apps/desktop    Interface React + shell desktop Tauri (macOS, Windows, Linux)
apps/mobile     Shell Tauri Android (reutiliza a interface desktop)
apps/website    Site no GitHub Pages e Demo ao vivo
crates/         Workspace Rust (ice-app, ice-config, ice-core, ice-engine, ice-subscription, ice-proxy-sys, ice-tun-sys, ice-elevate, ice-helper, ice-types, ...)
```

## Documentação

| Documento | Escopo |
|---|---|
| [`docs/architecture.md`](docs/architecture.md) | Arquitetura, limites dos módulos e fluxo de dados |
| [`docs/mcp.md`](docs/mcp.md) | Configuração do MCP no desktop, ferramentas e modelo de segurança |
| [`docs/tun.md`](docs/tun.md) | Captura TUN, elevação, recuperação e limites por plataforma |
| [`docs/testing.md`](docs/testing.md) | Gates locais/CI e testes manuais ao vivo |
| [`docs/release-process.md`](docs/release-process.md) | Versionamento, assinatura, empacotamento e publicação |
| [`CHANGELOG.md`](CHANGELOG.md) | Histórico de versões |

Estes documentos técnicos estão atualmente disponíveis apenas em inglês.

## Licença

[GPL-3.0-or-later](LICENSE). Núcleo sing-box incluído: [GPL-3.0-or-later](third_party/sing-box/LICENSE) com restrição de nomenclatura upstream. Avisos de terceiros: [NOTICE](NOTICE).

<div align="center">
<br>
<sub>Desenvolvido com Tauri, React e sing-box.</sub>
</div>
