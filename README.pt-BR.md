<div align="center">

<a href="https://yilong-musk.github.io/ice-box/">
  <img src="docs/brand/switch-logo/assets/logo.gif" alt="Logo do ice-box" width="144">
</a>

# ice-box

**A melhor sing-box GUI.**

Um cliente de proxy leve para macOS, Windows, Linux e Android.<br>
Fácil de configurar, rápido para iniciar, discreto enquanto roda e amigável para Agentes — construído sobre um núcleo [sing-box](https://github.com/SagerNet/sing-box).

[![Release](https://img.shields.io/github/v/release/yilong-musk/ice-box?style=flat-square&label=release)](https://github.com/yilong-musk/ice-box/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/yilong-musk/ice-box/ci.yml?branch=main&style=flat-square&label=CI)](https://github.com/yilong-musk/ice-box/actions/workflows/ci.yml)
[![Downloads](https://img.shields.io/github/downloads/yilong-musk/ice-box/total?style=flat-square)](https://github.com/yilong-musk/ice-box/releases)
[![Platforms](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux%20%7C%20Android-007ec6?style=flat-square)](#instalação)
[![Core](https://img.shields.io/badge/sing--box-1.14.4-6f42c1?style=flat-square)](https://github.com/SagerNet/sing-box)
[![License](https://img.shields.io/github/license/yilong-musk/ice-box?style=flat-square)](LICENSE)

[**Download**](https://github.com/yilong-musk/ice-box/releases/latest) · [**Demo ao vivo**](https://yilong-musk.github.io/ice-box/) · [Instalação](#instalação) · [TUN](#modo-tun) · [Documentação](#documentação) · [Changelog](CHANGELOG.md)

[Bahasa Indonesia](README.id.md) · [Deutsch](README.de.md) · [English](README.md) · [Español](README.es.md) · [Français](README.fr.md) · **Português (BR)** · [Tiếng Việt](README.vi.md) · [Türkçe](README.tr.md)<br>[Русский](README.ru.md) · [Українська](README.uk.md) · [العربية](README.ar.md) · [فارسی](README.fa.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [简体中文](README.zh-CN.md) · [繁體中文](README.zh-TW.md)

<br>

<a href="https://yilong-musk.github.io/ice-box/">
  <img src="docs/images/hero.png" alt="ice-box no desktop e Android: status do proxy, seletores de modo, nó de saída, gráfico de tráfego em tempo real e o Início voltado para celular com a lista de nós" width="880">
</a>

<sub>A <a href="https://yilong-musk.github.io/ice-box/">Demo ao vivo</a> executa as interfaces desktop e Android no seu navegador usando um backend simulado.</sub>

</div>

## Recursos

- **Assinaturas.** Listas em sing-box JSON, Clash e links de compartilhamento são detectadas automaticamente, e um arquivo de configuração local do sing-box também pode ser importado. O app desktop pode atualizar perfis automaticamente; o Android atualiza ao importar ou ao atualizar manualmente. O tráfego e a data de expiração informados pelo provedor aparecem em cada assinatura.
- **Roteamento.** Modos **Regra**, **Global** e **Direto**, um interruptor para cada regra da assinatura e suas próprias regras personalizadas por cima.
- **Nós.** Escolha uma saída por grupo de estratégia, teste a latência de um nó ou de uma lista inteira de uma vez e fixe seus favoritos.
- **Conexões.** No desktop (macOS, Windows e Linux), acompanhe cada conexão ativa: destino, regra correspondente, saída utilizada e quanto já transferiu. Pesquise, ordene e encerre uma ou todas elas.
- **Captura.** Proxy do sistema ou TUN no desktop; VPN do sistema no Android, com roteamento por app (seguir as regras, proxy, direto ou bloquear).
- **Bandeja.** Inicie e pare o serviço de proxy e troque o modo, a assinatura ou o nó de saída sem abrir a janela. No macOS, o item da barra de menus também exibe o tráfego em tempo real e o menu executa testes de latência.
- **Acesso de Agentes.** Um serviço MCP local opcional permite que um Agente opere a mesma sessão em execução.
- **Manutenção discreta.** Iniciar no login, atualizações no app verificadas por assinatura, leituras de tráfego e memória em tempo real no Início, 16 idiomas com suporte completo a RTL (Bahasa Indonesia, Deutsch, English, Español, Français, Português (BR), Tiếng Việt, Türkçe, Русский, Українська, العربية, فارسی, 日本語, 한국어, 简体中文, 繁體中文), temas claro e escuro.

## Instalação

Baixe o instalador para sua plataforma na [versão mais recente](https://github.com/yilong-musk/ice-box/releases/latest).

| Plataforma | Requisitos | Instalador | Primeira execução |
|---|---|---|---|
| **macOS** (Apple Silicon) | macOS 12 ou posterior | `ice-box_<version>_aarch64.dmg` | Assinado apenas de forma ad-hoc (sem Developer ID, sem notarização), portanto a primeira execução exibe um aviso de **desenvolvedor não identificado**. Permita em **Ajustes do Sistema → Privacidade e Segurança → Abrir Mesmo Assim** (macOS 15+), clique com o botão direito em **Abrir** (versões anteriores) ou execute `xattr -dr com.apple.quarantine /Applications/ice-box.app`. |
| **Windows** (x64) | Windows 10 ou posterior | `ice-box_<version>_x64-setup.exe` | Instalador NSIS por usuário; não exige privilégios de administrador. Não possui assinatura Authenticode, então o SmartScreen pode pedir confirmação. |
| **Linux** (x64 / arm64) | Ubuntu 22.04+ / distros modernas | `ice-box_<version>_amd64.AppImage` / `aarch64.AppImage`<br>`ice-box_<version>_amd64.deb` / `arm64.deb` | Torne o AppImage executável (`chmod +x`) ou instale o `.deb` com `sudo apt install ./ice-box_*.deb`. O ícone na bandeja do GNOME requer a extensão AppIndicator (`gnome-shell-extension-appindicator`). Em Wayland + NVIDIA, o ice-box define automaticamente `WEBKIT_DISABLE_DMABUF_RENDERER=1` caso ainda não esteja configurado. |
| **Android** (arm64) | Android 14 ou posterior | `ice-box_<version>_android_arm64.apk` | Assinado com o keystore do projeto. Não está listado no Google Play; o Android solicitará permissão para instalar desta fonte. |

**Atualização.** No macOS, Windows (0.1.5+) e instalações Linux via AppImage, o ice-box verifica o GitHub Releases em segundo plano e instala atualizações verificadas por assinatura em **Ajustes → Atualizações do app**; instalações mais antigas no macOS/Windows precisam de uma atualização manual primeiro. Instalações Linux `.deb` consultam o mesmo catálogo e abrem a página de download da versão para que as atualizações continuem gerenciadas pelo `apt` / `dpkg`. No Android, **Ajustes → Atualizações do app** baixa o APK arm64 e o entrega ao instalador do sistema para confirmação.

## Início rápido

1. **Importe uma assinatura.** Abra **Assinaturas**, cole a URL e clique em **Importar**; o formato é detectado automaticamente. Para usar um arquivo de configuração local do sing-box (JSON), escolha **Arquivo de config**; ele não tem URL remota e, portanto, não pode ser atualizado.
2. **Escolha um modo e um nó.** Em **Início**, escolha **Regra**, **Global** ou **Direto**. No desktop, abra **Nós** para escolher uma saída e testar a latência; no Android, use a lista de nós em **Início**.
3. **Inicie o serviço de proxy.** Pressione o botão de energia. O desktop aplica o proxy do sistema ou inicia o TUN quando ativado; o Android conecta a VPN após a solicitação de permissão do sistema.

Ainda não tem uma assinatura? O ice-box inicia no modo somente direto, permitindo explorar o núcleo, a captura e a interface antes de importar qualquer coisa.

## Iniciar no login

No macOS, Windows e Linux, ative **Ajustes → Inicialização → Iniciar no login**. Ao iniciar no login, o app permanece na bandeja com a janela oculta, inicia o núcleo e restaura o último estado do proxy.

Desative usando o mesmo interruptor: desabilitar a entrada em **Itens de Início** no macOS não limpa a configuração. Instale o aplicativo antes de ativar essa opção no macOS, pois a execução direta a partir da imagem de disco é recusada. No Linux, a entrada é gravada em `~/.config/autostart/com.yilong-musk.icebox.desktop` (registrando `$APPIMAGE` quando executado a partir de um AppImage).

O Android não possui item de login. **Ajustes → Sistema** explica a exceção de otimização de bateria, o DNS privado estrito e a VPN sempre ativa.

## Modo TUN

No macOS, Windows e Linux, o TUN captura o tráfego na camada de rede, cobrindo aplicativos que nunca leem o proxy do sistema. O TUN no Windows transporta apenas IPv4 TCP (sem UDP/QUIC nem IPv6) e exige uma conta de Administrador; o proxy do sistema continua sendo a alternativa nesses casos. O TUN no Linux usa Polkit (`pkexec`) para instalar um daemon systemd `ice-helper` e gerencia o DNS via `systemd-resolved` (`resolvectl`) ou um `/etc/resolv.conf` estático. Consulte [`docs/tun.md`](docs/tun.md) para configuração, comportamento da chave, cobertura por plataforma e limitações. O Android usa a VPN do sistema; veja [`docs/architecture.md`](docs/architecture.md#clients).

## Acesso de Agentes com MCP

O aplicativo desktop (macOS, Windows e Linux) expõe um serviço MCP local opcional. Abra **MCP** entre **Conexões** e **Ajustes**, ative-o e copie a configuração de cliente gerada. O Agente compartilha os ajustes, assinaturas, dados, núcleo e estado do proxy da sessão em execução em vez de iniciar outro runtime.

- **Conjunto completo de ferramentas.** Status, ajustes, tráfego, assinaturas, nós e grupos, regras, conexões ativas, logs, diagnósticos, controle do proxy e recuperação protegida do TUN. Os resultados trazem conteúdo estruturado quando disponível e preservam os códigos de erro da aplicação.
- **Seguro por padrão.** O MCP escuta apenas em loopback autenticado com um token de controle aleatório que nunca é incluído na configuração de cliente copiada.
- **Ponte stdio integrada.** A configuração gerada inicia o mesmo executável em modo ponte, dispensando daemon separado ou adaptador de protocolo.

O MCP está disponível apenas no desktop: o Android não possui MCP e a demo do site usa um serviço simulado. Consulte [`docs/mcp.md`](docs/mcp.md) para configuração, ciclo de vida e limites de segurança.

## Desenvolvimento

Rust (a versão fixada em `rust-toolchain.toml`, instalada pelo rustup no primeiro uso), Node.js 22, Xcode Command Line Tools no macOS e pacotes de desenvolvimento GTK/WebKit (`libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`, `libssl-dev`) no Linux. Builds para Android também exigem JDK 17, Go e um Android SDK (`ANDROID_HOME`) com os pacotes instalados por `scripts/ci-install-android-sdk.sh`.

```bash
cd apps/desktop && npm install && cd ../..
npm run fetch-singbox   # baixa o núcleo sing-box fixado para este host
npm run dev
```

| Comando | Finalidade |
|---|---|
| `npm run dev:mac-arm64` / `dev:mac-x64` / `dev:win` / `dev:linux-x64` / `dev:linux-arm64` | Executar para um alvo de host específico |
| `npm run gate` | Gate completo do workspace: fmt, clippy, testes Rust, checagens de tipo, builds, smoke tests, fixtures |
| `npm run fetch-singbox && npm run build` | Gerar o `.dmg` para macOS |
| `npm run fetch-singbox -- win && npm run build:win` | Gerar o instalador NSIS para Windows |
| `npm run fetch-singbox -- linux-x64 && npm run build:linux` | Gerar `.deb` e `.AppImage` para Linux (passe `linux-arm64` em arm64) |
| `npm run acceptance` / `acceptance:tun` / `acceptance:win` / `acceptance:linux` / `acceptance:linux-tun` | Gates de aceitação ao vivo em um host real |
| `bash scripts/build-android-apk.sh debug arm64` | APK de depuração para um dispositivo arm64 |
| `bash scripts/build-android-apk.sh debug amd64` | APK de depuração para o emulador |

```
apps/desktop    Interface React + shell Tauri para macOS, Windows e Linux
apps/mobile     Shell Android; reutiliza a interface desktop
apps/website    Site no GitHub Pages e Demo ao vivo
crates/         Workspace Rust (crates principais): ice-app, ice-config, ice-core, ice-engine, ice-subscription, ice-proxy-sys, ice-tun-sys, ice-elevate, ice-helper, ice-types, ...
```

## Documentação

| Documento | O que você encontrará |
|---|---|
| [`docs/architecture.md`](docs/architecture.md) | Estrutura do sistema, responsabilidades dos componentes, fluxo de dados e limites de design |
| [`docs/mcp.md`](docs/mcp.md) | Configuração do MCP no desktop, modelo de runtime compartilhado, ferramentas e segurança |
| [`docs/tun.md`](docs/tun.md) | Comportamento do TUN, recuperação, configuração por plataforma e limites |
| [`docs/testing.md`](docs/testing.md) | Cobertura dos gates locais e de CI, testes manuais ao vivo |
| [`docs/release-process.md`](docs/release-process.md) | Versionamento, assinatura, empacotamento e publicação |
| [`CHANGELOG.md`](CHANGELOG.md) | Histórico de versões |

Estes documentos técnicos estão atualmente disponíveis apenas em inglês.

## Licença

O ice-box é software livre, distribuído sob a [Licença Pública Geral GNU v3.0 ou posterior](LICENSE). O núcleo sing-box incluído também é licenciado sob [GPL-3.0-or-later](third_party/sing-box/LICENSE), com sua restrição de nomenclatura upstream; consulte [NOTICE](NOTICE) para todos os avisos de terceiros.

<div align="center">
<br>
<sub>Desenvolvido com Tauri, React e sing-box.</sub>
</div>
