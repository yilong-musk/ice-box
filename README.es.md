<div align="center">

<a href="https://yilong-musk.github.io/ice-box/">
  <img src="docs/brand/switch-logo/assets/logo.gif" alt="ice-box logo" width="144">
</a>

# ice-box

**La mejor sing-box GUI.**

Un cliente proxy ligero para macOS, Windows, Linux y Android.<br>
Fácil de configurar, rápido al iniciar, discreto una vez en marcha y compatible con agentes — construido sobre un núcleo [sing-box](https://github.com/SagerNet/sing-box).

[![Release](https://img.shields.io/github/v/release/yilong-musk/ice-box?style=flat-square&label=release)](https://github.com/yilong-musk/ice-box/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/yilong-musk/ice-box/ci.yml?branch=main&style=flat-square&label=CI)](https://github.com/yilong-musk/ice-box/actions/workflows/ci.yml)
[![Downloads](https://img.shields.io/github/downloads/yilong-musk/ice-box/total?style=flat-square)](https://github.com/yilong-musk/ice-box/releases)
[![Platforms](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux%20%7C%20Android-007ec6?style=flat-square)](#instalación)
[![Core](https://img.shields.io/badge/sing--box-1.14.4-6f42c1?style=flat-square)](https://github.com/SagerNet/sing-box)
[![License](https://img.shields.io/github/license/yilong-musk/ice-box?style=flat-square)](LICENSE)

[**Descargar**](https://github.com/yilong-musk/ice-box/releases/latest) · [**Demo en vivo**](https://yilong-musk.github.io/ice-box/) · [Instalación](#instalación) · [TUN](#modo-tun) · [Documentación](#documentación) · [Registro de cambios](CHANGELOG.md)

[Bahasa Indonesia](README.id.md) · [Deutsch](README.de.md) · [English](README.md) · **Español** · [Français](README.fr.md) · [Português (BR)](README.pt-BR.md) · [Tiếng Việt](README.vi.md) · [Türkçe](README.tr.md)<br>[Русский](README.ru.md) · [Українська](README.uk.md) · [العربية](README.ar.md) · [فارسی](README.fa.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [简体中文](README.zh-CN.md) · [繁體中文](README.zh-TW.md)

<br>

<a href="https://yilong-musk.github.io/ice-box/">
  <img src="docs/images/hero.png" alt="ice-box en escritorio y Android: estado del proxy, interruptores de modo, nodo de salida, gráfico de tráfico en vivo y la pantalla Inicio optimizada para móvil con la lista de nodos" width="880">
</a>

<sub>La <a href="https://yilong-musk.github.io/ice-box/">Demo en vivo</a> ejecuta las interfaces de escritorio y Android en tu navegador con un backend simulado.</sub>

</div>

## Funciones

- **Suscripciones.** Detecta automáticamente sing-box JSON, Clash y listas de enlaces compartidos, y también permite importar un archivo de configuración local de sing-box. En escritorio los perfiles pueden actualizarse automáticamente; en Android se actualizan al importar o mediante actualización manual. Cada suscripción muestra el tráfico y la fecha de vencimiento del proveedor.
- **Enrutamiento.** Modos **Reglas**, **Global** y **Directo**, un interruptor individual para cada regla de suscripción y tus propias reglas personalizadas encima.
- **Nodos.** Elige una salida por cada grupo de estrategia, prueba la latencia de un nodo o de toda una lista a la vez y fija tus favoritos.
- **Conexiones.** En escritorio (macOS, Windows y Linux), observa cada conexión activa: su destino, la regla que coincidió, la salida utilizada y cuánto tráfico ha transferido. Busca, ordena y cierra una o todas las conexiones.
- **Captura.** Proxy del sistema o TUN en escritorio; VPN del sistema en Android, con enrutamiento por app (seguir reglas, proxy, directo o bloquear).
- **Bandeja.** Inicia y detiene el servicio proxy y cambia de modo, suscripción o nodo de salida sin abrir la ventana. En macOS, el elemento de la barra de menús también muestra el tráfico en vivo y el menú permite ejecutar pruebas de latencia.
- **Acceso para agentes.** Un servicio MCP local opcional permite que un agente opere sobre la misma sesión en ejecución.
- **Mantenimiento sin esfuerzo.** Abrir al iniciar sesión, actualizaciones en la app verificadas por firma, lecturas de tráfico y memoria en tiempo real en Inicio, 16 idiomas con soporte RTL completo (Bahasa Indonesia, Deutsch, English, Español, Français, Português (BR), Tiếng Việt, Türkçe, Русский, Українська, العربية, فارسی, 日本語, 한국어, 简体中文, 繁體中文), y temas claro y oscuro.

## Instalación

Descarga el instalador para tu plataforma desde la [última versión](https://github.com/yilong-musk/ice-box/releases/latest).

| Plataforma | Requisitos | Instalador | Primer inicio |
|---|---|---|---|
| **macOS** (Apple Silicon) | macOS 12 o posterior | `ice-box_<version>_aarch64.dmg` | Solo cuenta con firma ad-hoc (sin Developer ID ni notarización), por lo que al abrirla por primera vez muestra un aviso de **desarrollador no identificado**. Permítela en **Ajustes del Sistema → Privacidad y seguridad → Abrir igualmente** (macOS 15+), con clic derecho y **Abrir** (versiones anteriores) o ejecutando `xattr -dr com.apple.quarantine /Applications/ice-box.app`. |
| **Windows** (x64) | Windows 10 o posterior | `ice-box_<version>_x64-setup.exe` | Instalador NSIS por usuario; no requiere permisos de administrador. No tiene firma Authenticode, por lo que SmartScreen puede pedirte confirmación. |
| **Linux** (x64 / arm64) | Ubuntu 22.04+ / distribuciones modernas | `ice-box_<version>_amd64.AppImage` / `aarch64.AppImage`<br>`ice-box_<version>_amd64.deb` / `arm64.deb` | Haz ejecutable el AppImage (`chmod +x`) o instala el `.deb` con `sudo apt install ./ice-box_*.deb`. El icono de bandeja en GNOME necesita la extensión AppIndicator (`gnome-shell-extension-appindicator`). En Wayland + NVIDIA, ice-box establece automáticamente `WEBKIT_DISABLE_DMABUF_RENDERER=1` salvo que ya esté configurado. |
| **Android** (arm64) | Android 14 o posterior | `ice-box_<version>_android_arm64.apk` | Firmado con el almacén de claves del proyecto. No está publicado en Google Play; Android pedirá permiso para instalar desde esta fuente. |

**Actualizaciones.** En macOS, Windows (0.1.5+) e instalaciones Linux AppImage, ice-box consulta GitHub Releases en segundo plano e instala actualizaciones verificadas por firma desde **Ajustes → Actualizaciones de la app**; las instalaciones anteriores de macOS/Windows requieren primero una actualización manual. Las instalaciones `.deb` en Linux consultan el mismo catálogo y abren la página de descarga de la versión para que las actualizaciones sigan gestionándose mediante `apt` / `dpkg`. En Android, **Ajustes → Actualizaciones de la app** descarga el APK arm64 y lo entrega al instalador del sistema para su confirmación.

## Inicio rápido

1. **Importa una suscripción.** Abre **Suscripciones**, pega la URL y pulsa **Importar**; el formato se detecta automáticamente. Para usar en su lugar un archivo de configuración local de sing-box (JSON), elige **Archivo de configuración**; no tiene URL remota, por lo que no se puede actualizar.
2. **Elige un modo y un nodo.** En **Inicio**, selecciona **Reglas**, **Global** o **Directo**. En escritorio, abre **Nodos** para elegir una salida y probar su latencia; en Android, usa la lista de nodos en **Inicio**.
3. **Inicia el servicio proxy.** Pulsa el botón de encendido. En escritorio se aplica el proxy del sistema o se inicia TUN si está activado; en Android se conecta la VPN tras aceptar el permiso del sistema.

¿Aún no tienes suscripción? ice-box arranca en modo solo directo, de modo que puedes explorar el núcleo, la captura y la interfaz antes de importar nada.

## Abrir al iniciar sesión

En macOS, Windows y Linux, activa **Ajustes → Inicio → Abrir al iniciar sesión**. Al iniciarse con la sesión, la app permanece en la bandeja con la ventana oculta, arranca el núcleo y restaura el último estado del proxy.

Desactívalo con el mismo interruptor: deshabilitar la entrada en **Ítems de inicio de sesión** de macOS no borra el ajuste. Instala la app antes de activarlo en macOS, ya que no se permite ejecutarla directamente desde la imagen de disco. En Linux, la entrada se escribe en `~/.config/autostart/com.yilong-musk.icebox.desktop` (registrando `$APPIMAGE` cuando se ejecuta desde un AppImage).

Android no tiene elementos de inicio de sesión. **Ajustes → Sistema** explica la exención de optimización de batería, el DNS privado estricto y la VPN siempre activa.

## Modo TUN

En macOS, Windows y Linux, TUN captura el tráfico en la capa de red, cubriendo aplicaciones que nunca leen el proxy del sistema. El TUN de Windows solo transporta TCP sobre IPv4 (sin UDP/QUIC ni IPv6) y requiere una cuenta de Administrador; el proxy del sistema sigue siendo la alternativa allí. El TUN de Linux utiliza Polkit (`pkexec`) para instalar un demonio `ice-helper` de systemd y gestiona el DNS mediante `systemd-resolved` (`resolvectl`) o un `/etc/resolv.conf` estático. Consulta [`docs/tun.md`](docs/tun.md) para la configuración, el comportamiento de conmutación, la cobertura por plataforma y las limitaciones. Android usa la VPN del sistema; consulta [`docs/architecture.md`](docs/architecture.md#clients).

## Acceso para agentes con MCP

La app de escritorio (macOS, Windows y Linux) expone un servicio MCP local opcional. Abre **MCP** entre **Conexiones** y **Ajustes**, actívalo y copia la configuración de cliente generada. El agente comparte los ajustes, las suscripciones, los datos, el núcleo y el estado del proxy de la sesión en ejecución en lugar de iniciar otro entorno de ejecución.

- **Conjunto completo de herramientas.** Estado, ajustes, tráfico, suscripciones, nodos y grupos, reglas, conexiones activas, registros, diagnósticos, control del proxy y recuperación protegida de TUN. Los resultados incluyen contenido estructurado cuando está disponible y conservan los códigos de error de la aplicación.
- **Seguro por defecto.** MCP escucha únicamente en loopback autenticado con un token de control aleatorio que nunca se incluye en la configuración de cliente copiada.
- **Puente stdio integrado.** La configuración generada lanza el mismo ejecutable en modo puente, por lo que no se necesita ningún demonio ni adaptador de protocolo adicional.

MCP es exclusivo de escritorio: Android no incluye MCP y la demo web utiliza un servicio simulado. Consulta [`docs/mcp.md`](docs/mcp.md) para ver la configuración, el ciclo de vida y los límites de seguridad.

## Desarrollo

Rust (la versión fijada en `rust-toolchain.toml`, que rustup instala en el primer uso), Node.js 22, Xcode Command Line Tools en macOS y paquetes de desarrollo de GTK/WebKit (`libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`, `libssl-dev`) en Linux. Las compilaciones de Android también requieren JDK 17, Go y un Android SDK (`ANDROID_HOME`) con los paquetes instalados por `scripts/ci-install-android-sdk.sh`.

```bash
cd apps/desktop && npm install && cd ../..
npm run fetch-singbox   # descarga el núcleo sing-box fijado para este equipo
npm run dev
```

| Comando | Propósito |
|---|---|
| `npm run dev:mac-arm64` / `dev:mac-x64` / `dev:win` / `dev:linux-x64` / `dev:linux-arm64` | Ejecutar para un objetivo de host específico |
| `npm run gate` | Compuerta completa del espacio de trabajo: fmt, clippy, pruebas de Rust, comprobación de tipos, compilaciones, pruebas de humo y fixtures |
| `npm run fetch-singbox && npm run build` | Compilar el `.dmg` de macOS |
| `npm run fetch-singbox -- win && npm run build:win` | Compilar el instalador NSIS de Windows |
| `npm run fetch-singbox -- linux-x64 && npm run build:linux` | Compilar `.deb` y `.AppImage` de Linux (pasa `linux-arm64` en arm64) |
| `npm run acceptance` / `acceptance:tun` / `acceptance:win` / `acceptance:linux` / `acceptance:linux-tun` | Compuertas de aceptación en vivo en un equipo real |
| `bash scripts/build-android-apk.sh debug arm64` | APK de depuración para un dispositivo arm64 |
| `bash scripts/build-android-apk.sh debug amd64` | APK de depuración para el emulador |

```
apps/desktop    Interfaz React + contenedor Tauri para macOS, Windows y Linux
apps/mobile     Contenedor Android; reutiliza la interfaz de escritorio
apps/website    Sitio de GitHub Pages y Demo en vivo
crates/         Espacio de trabajo Rust (crates principales): ice-app, ice-config, ice-core, ice-engine, ice-subscription, ice-proxy-sys, ice-tun-sys, ice-elevate, ice-helper, ice-types, ...
```

## Documentación

| Documento | Qué encontrarás |
|---|---|
| [`docs/architecture.md`](docs/architecture.md) | Estructura del sistema, responsabilidades de los componentes, flujo de datos y límites de diseño |
| [`docs/mcp.md`](docs/mcp.md) | Configuración de MCP en escritorio, modelo de entorno de ejecución compartido, herramientas y seguridad |
| [`docs/tun.md`](docs/tun.md) | Comportamiento de TUN, recuperación, configuración por plataforma y límites |
| [`docs/testing.md`](docs/testing.md) | Cobertura de compuertas locales y de CI, pruebas manuales en vivo |
| [`docs/release-process.md`](docs/release-process.md) | Versionado, firmado, empaquetado y publicación |
| [`CHANGELOG.md`](CHANGELOG.md) | Historial de versiones |

La documentación técnica anterior está disponible actualmente en inglés.

## Licencia

ice-box es software libre, publicado bajo la [Licencia Pública General de GNU v3.0 o posterior](LICENSE). El núcleo sing-box incluido también está licenciado bajo [GPL-3.0-or-later](third_party/sing-box/LICENSE), con su restricción de denominación original; consulta [NOTICE](NOTICE) para ver todos los avisos de terceros.

<div align="center">
<br>
<sub>Construido con Tauri, React y sing-box.</sub>
</div>
