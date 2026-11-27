<div align="center">

<a href="https://yilong-musk.github.io/ice-box/">
  <img src="docs/brand/switch-logo/assets/logo.gif" alt="ice-box logo" width="144">
</a>

# ice-box

**En iyi sing-box GUI.**

macOS, Windows, Linux ve Android için hafif bir proxy istemcisi.<br>
Kurulumu kolay, açılışı hızlı, çalışırken sessiz ve Agent dostu — [sing-box](https://github.com/SagerNet/sing-box) çekirdeği üzerine kurulu.

[![Release](https://img.shields.io/github/v/release/yilong-musk/ice-box?style=flat-square&label=release)](https://github.com/yilong-musk/ice-box/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/yilong-musk/ice-box/ci.yml?branch=main&style=flat-square&label=CI)](https://github.com/yilong-musk/ice-box/actions/workflows/ci.yml)
[![Downloads](https://img.shields.io/github/downloads/yilong-musk/ice-box/total?style=flat-square)](https://github.com/yilong-musk/ice-box/releases)
[![Platforms](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux%20%7C%20Android-007ec6?style=flat-square)](#kurulum)
[![Core](https://img.shields.io/badge/sing--box-1.14.4-6f42c1?style=flat-square)](https://github.com/SagerNet/sing-box)
[![License](https://img.shields.io/github/license/yilong-musk/ice-box?style=flat-square)](LICENSE)

[**İndir**](https://github.com/yilong-musk/ice-box/releases/latest) · [**Canlı Demo**](https://yilong-musk.github.io/ice-box/) · [Kurulum](#kurulum) · [TUN](#tun-modu) · [Belgeler](#belgeler) · [Değişiklik Günlüğü](CHANGELOG.md)

[Bahasa Indonesia](README.id.md) · [Deutsch](README.de.md) · [English](README.md) · [Español](README.es.md) · [Français](README.fr.md) · [Português (BR)](README.pt-BR.md) · [Tiếng Việt](README.vi.md) · **Türkçe**<br>[Русский](README.ru.md) · [Українська](README.uk.md) · [العربية](README.ar.md) · [فارسی](README.fa.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [简体中文](README.zh-CN.md) · [繁體中文](README.zh-TW.md)

<br>

<a href="https://yilong-musk.github.io/ice-box/">
  <img src="docs/images/hero.png" alt="Masaüstünde ve Android'de ice-box: proxy durumu, mod anahtarları, çıkış düğümü, canlı trafik grafiği ve düğüm listesiyle mobil öncelikli Ana Sayfa" width="880">
</a>

<sub><a href="https://yilong-musk.github.io/ice-box/">Canlı Demo</a>, masaüstü ve Android arayüzlerini tarayıcınızda simüle edilmiş bir arka uç üzerinde çalıştırır.</sub>

</div>

## Özellikler

- **Abonelikler.** sing-box JSON, Clash ve paylaşım bağlantısı listeleri otomatik olarak algılanır; yerel bir sing-box yapılandırma dosyası da içe aktarılabilir. Masaüstü profilleri otomatik olarak güncelleyebilir; Android ise içe aktarma veya manuel yenileme sırasında günceller. Sağlayıcı trafiği ve bitiş tarihi her abonelikte gösterilir.
- **Yönlendirme.** **Kural**, **Genel** ve **Doğrudan** modları, her abonelik kuralı için ayrı anahtar ve üzerine ekleyebileceğiniz kendi özel kurallarınız.
- **Düğümler.** Her strateji grubu için bir çıkış seçin, tek bir düğümün veya tüm listenin gecikmesini test edin ve favorilerinizi sabitleyin.
- **Bağlantılar.** Masaüstünde (macOS, Windows ve Linux) her canlı bağlantıyı izleyin: nereye gittiğini, hangi kuralın eşleştiğini, hangi çıkışı kullandığını ve ne kadar veri aktardığını görün. Arayın, sıralayın ve tek bir bağlantıyı ya da tümünü kapatın.
- **Yakalama.** Masaüstünde sistem proxy veya TUN; Android'de ise uygulama bazlı yönlendirme (kuralları izle, proxy, doğrudan veya engelle) sunan sistem VPN'i.
- **Sistem tepsisi.** Pencereyi açmadan proxy hizmetini başlatıp durdurun; modu, aboneliği veya çıkış düğümünü değiştirin. macOS'te menü çubuğu öğesi canlı trafiği de gösterir ve menü üzerinden gecikme testleri çalıştırılabilir.
- **Agent erişimi.** İsteğe bağlı yerel MCP hizmeti, bir Agent'ın çalışmakta olan aynı oturumu yönetmesini sağlar.
- **Sessiz bakım.** Oturum açıldığında başlatma, imza doğrulamalı uygulama içi güncellemeler, Ana Sayfa'da canlı trafik ve bellek göstergeleri, tam RTL destekli 16 dil (Bahasa Indonesia, Deutsch, English, Español, Français, Português (BR), Tiếng Việt, Türkçe, Русский, Українська, العربية, فارسی, 日本語, 한국어, 简体中文, 繁體中文), açık ve koyu temalar.

## Kurulum

Platformunuza uygun yükleyiciyi [en son sürümden](https://github.com/yilong-musk/ice-box/releases/latest) indirin.

| Platform | Gereksinim | Yükleyici | İlk açılış |
|---|---|---|---|
| **macOS** (Apple Silicon) | macOS 12 veya üzeri | `ice-box_<version>_aarch64.dmg` | Yalnızca ad-hoc imzalıdır (Developer ID yok, noter onaylı değil); bu nedenle ilk açılışta **tanınmayan geliştirici** uyarısı görünür. **Sistem Ayarları → Gizlilik ve Güvenlik → Yine de Aç** üzerinden izin verin (macOS 15+), sağ tıklayıp **Aç** seçeneğini kullanın (eski sürümler) veya `xattr -dr com.apple.quarantine /Applications/ice-box.app` komutunu çalıştırın. |
| **Windows** (x64) | Windows 10 veya üzeri | `ice-box_<version>_x64-setup.exe` | Kullanıcı bazlı NSIS yükleyicisi; yönetici hakları gerekmez. Authenticode imzası bulunmadığından SmartScreen onay isteyebilir. |
| **Linux** (x64 / arm64) | Ubuntu 22.04+ / modern dağıtımlar | `ice-box_<version>_amd64.AppImage` / `aarch64.AppImage`<br>`ice-box_<version>_amd64.deb` / `arm64.deb` | AppImage dosyasını çalıştırılabilir yapın (`chmod +x`) veya `.deb` paketini `sudo apt install ./ice-box_*.deb` ile kurun. GNOME sistem tepsisi simgesi için AppIndicator uzantısı (`gnome-shell-extension-appindicator`) gerekir. Wayland + NVIDIA üzerinde, daha önce ayarlanmadıysa ice-box otomatik olarak `WEBKIT_DISABLE_DMABUF_RENDERER=1` değişkenini ayarlar. |
| **Android** (arm64) | Android 14 veya üzeri | `ice-box_<version>_android_arm64.apk` | Proje anahtar deposuyla (keystore) imzalanmıştır. Google Play'de yer almaz; Android bu kaynaktan kurulum için izin ister. |

**Güncelleme.** macOS, Windows (0.1.5+) ve Linux AppImage kurulumlarında ice-box arka planda GitHub Releases'ı denetler ve imza doğrulamalı güncellemeleri **Ayarlar → Uygulama Güncellemeleri** üzerinden yükler; daha eski macOS/Windows kurulumlarının önce bir kez manuel olarak yükseltilmesi gerekir. Linux `.deb` kurulumları aynı kataloğu denetler ve yükseltmelerin `apt` / `dpkg` tarafından yönetilmeye devam etmesi için sürüm indirme sayfasını açar. Android'de **Ayarlar → Uygulama Güncellemeleri** arm64 APK dosyasını indirir ve onay için sistem yükleyicisine aktarır.

## Hızlı başlangıç

1. **Bir abonelik içe aktarın.** **Abonelikler** bölümünü açın, URL'yi yapıştırın ve **İçe Aktar**'a basın; biçim otomatik olarak algılanır. Bunun yerine yerel bir sing-box yapılandırma dosyası (JSON) kullanmak için **Yapılandırma dosyası** seçeneğini seçin; uzak bir URL'si olmadığından yenilenemez.
2. **Bir mod ve düğüm seçin.** **Ana Sayfa**'da **Kural**, **Genel** veya **Doğrudan** seçeneğini seçin. Masaüstünde bir çıkış seçmek ve gecikmesini test etmek için **Düğümler**'i açın; Android'de **Ana Sayfa**'daki düğüm listesini kullanın.
3. **Proxy hizmetini başlatın.** Güç düğmesine basın. Masaüstü sistem proxy'sini uygular veya etkinleştirildiyse TUN'u başlatır; Android sistem izin isteminden sonra VPN'e bağlanır.

Henüz bir aboneliğiniz yok mu? ice-box yalnızca doğrudan modda başlar; böylece herhangi bir şey içe aktarmadan önce çekirdeği, trafik yakalamayı ve arayüzü keşfedebilirsiniz.

## Oturum açıldığında başlat

macOS, Windows ve Linux'ta **Ayarlar → Başlangıç → Oturum açıldığında başlat** seçeneğini etkinleştirin. Oturum açılışında başlatıldığında pencere gizli kalarak sistem tepsisinde bekler, çekirdeği başlatır ve son proxy durumunu geri yükler.

Aynı anahtarla kapatın: macOS **Oturum Açma Öğeleri** altındaki girdiyi devre dışı bırakmak bu ayarı temizlemez. macOS'te etkinleştirmeden önce uygulamayı kurun; doğrudan disk görüntüsünden çalıştırma reddedilir. Linux'ta girdi `~/.config/autostart/com.yilong-musk.icebox.desktop` dosyasına yazılır (AppImage üzerinden çalıştırıldığında `$APPIMAGE` kaydedilir).

Android'de oturum açma öğesi yoktur. **Ayarlar → Sistem** bölümü pil optimizasyonu istisnasını, katı Özel DNS'i ve her zaman açık VPN'i açıklar.

## TUN Modu

macOS, Windows ve Linux'ta TUN trafiği ağ katmanında yakalar ve sistem proxy ayarını hiç okumayan uygulamaları da kapsar. Windows TUN yalnızca IPv4 TCP taşır (UDP/QUIC veya IPv6 desteklenmez) ve Yönetici hesabı gerektirir; orada sistem proxy alternatif olarak kalır. Linux TUN, bir systemd `ice-helper` arka plan hizmeti kurmak için Polkit (`pkexec`) kullanır ve DNS'i `systemd-resolved` (`resolvectl`) veya statik `/etc/resolv.conf` üzerinden yönetir. Kurulum, anahtar davranışı, platform kapsamı ve sınırlamalar için [`docs/tun.md`](docs/tun.md) belgesine bakın. Android sistem VPN'ini kullanır; bkz. [`docs/architecture.md`](docs/architecture.md#clients).

## MCP ile Agent erişimi

Masaüstü uygulaması (macOS, Windows ve Linux) isteğe bağlı bir yerel MCP hizmeti sunar. **Bağlantılar** ile **Ayarlar** arasındaki **MCP** sayfasını açın, hizmeti etkinleştirin ve oluşturulan istemci yapılandırmasını kopyalayın. Agent, yeni bir çalışma zamanı başlatmak yerine çalışmakta olan oturumun ayarlarını, aboneliklerini, verilerini, çekirdeğini ve proxy durumunu paylaşır.

- **Eksiksiz araç seti.** Durum, ayarlar, trafik, abonelikler, düğümler ve gruplar, kurallar, canlı bağlantılar, günlükler, tanılama, proxy kontrolü ve korumalı TUN kurtarma. Sonuçlar mevcut olduğunda yapılandırılmış içerik taşır ve uygulama hata kodlarını korur.
- **Varsayılan olarak güvenli.** MCP yalnızca kimliği doğrulanmış geri döngü (loopback) adresinde, kopyalanan istemci yapılandırmasına asla dahil edilmeyen rastgele bir kontrol belirteciyle dinler.
- **Yerleşik stdio köprüsü.** Oluşturulan yapılandırma aynı çalıştırılabilir dosyayı köprü modunda başlatır; ayrı bir arka plan hizmetine veya protokol bağdaştırıcısına gerek yoktur.

MCP yalnızca masaüstündedir: Android'de MCP yoktur ve web sitesi demosu simüle edilmiş bir hizmet kullanır. Kurulum, yaşam döngüsü ve güvenlik sınırları için [`docs/mcp.md`](docs/mcp.md) belgesine bakın.

## Geliştirme

Rust (`rust-toolchain.toml` içinde sabitlenmiş sürüm; rustup ilk kullanımda kurar), Node.js 22, macOS'te Xcode Command Line Tools ve Linux'ta GTK/WebKit geliştirme paketleri (`libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`, `libssl-dev`). Android derlemeleri ayrıca JDK 17, Go ve `scripts/ci-install-android-sdk.sh` tarafından kurulan paketlerle bir Android SDK (`ANDROID_HOME`) gerektirir.

```bash
cd apps/desktop && npm install && cd ../..
npm run fetch-singbox   # bu makine için sabitlenmiş sing-box çekirdeğini indir
npm run dev
```

| Komut | Amaç |
|---|---|
| `npm run dev:mac-arm64` / `dev:mac-x64` / `dev:win` / `dev:linux-x64` / `dev:linux-arm64` | Belirli bir hedef platforma göre çalıştır |
| `npm run gate` | Tam çalışma alanı kapısı: fmt, clippy, Rust testleri, tür denetimleri, derlemeler, smoke testleri, fixtures |
| `npm run fetch-singbox && npm run build` | macOS `.dmg` paketini derle |
| `npm run fetch-singbox -- win && npm run build:win` | Windows NSIS yükleyicisini derle |
| `npm run fetch-singbox -- linux-x64 && npm run build:linux` | Linux `.deb` ve `.AppImage` paketlerini derle (arm64 üzerinde `linux-arm64` geçin) |
| `npm run acceptance` / `acceptance:tun` / `acceptance:win` / `acceptance:linux` / `acceptance:linux-tun` | Gerçek bir makinede canlı kabul testleri |
| `bash scripts/build-android-apk.sh debug arm64` | arm64 cihaz için hata ayıklama APK'sı |
| `bash scripts/build-android-apk.sh debug amd64` | Emülatör için hata ayıklama APK'sı |

```
apps/desktop    macOS, Windows ve Linux için React arayüzü + Tauri kabuğu
apps/mobile     Android kabuğu; masaüstü arayüzünü yeniden kullanır
apps/website    GitHub Pages sitesi ve Canlı Demo
crates/         Rust çalışma alanı (ana crate'ler): ice-app, ice-config, ice-core, ice-engine, ice-subscription, ice-proxy-sys, ice-tun-sys, ice-elevate, ice-helper, ice-types, ...
```

## Belgeler

| Belge | İçerik |
|---|---|
| [`docs/architecture.md`](docs/architecture.md) | Sistem yapısı, bileşen sorumlulukları, veri akışı ve tasarım sınırları |
| [`docs/mcp.md`](docs/mcp.md) | Masaüstü MCP kurulumu, paylaşımlı çalışma zamanı modeli, araçlar ve güvenlik |
| [`docs/tun.md`](docs/tun.md) | TUN davranışı, kurtarma, platform yapılandırması ve sınırlamalar |
| [`docs/testing.md`](docs/testing.md) | Yerel ve CI kapı kapsamı, manuel canlı testler |
| [`docs/release-process.md`](docs/release-process.md) | Sürümleme, imzalama, paketleme ve yayınlama |
| [`CHANGELOG.md`](CHANGELOG.md) | Sürüm geçmişi |

Yukarıdaki teknik belgeler şu anda yalnızca İngilizcedir.

## Lisans

ice-box, [GNU Genel Kamu Lisansı v3.0 veya üzeri](LICENSE) altında yayınlanan özgür bir yazılımdır. Yerleşik sing-box çekirdeği de üst akış adlandırma kısıtlamasıyla birlikte [GPL-3.0-or-later](third_party/sing-box/LICENSE) lisansına sahiptir; tüm üçüncü taraf bildirimleri için [NOTICE](NOTICE) dosyasına bakın.

<div align="center">
<br>
<sub>Tauri, React ve sing-box ile geliştirilmiştir.</sub>
</div>
