<div align="center">

<a href="https://yilong-musk.github.io/ice-box/">
  <img src="docs/brand/switch-logo/assets/logo.gif" alt="ice-box logo" width="144">
</a>

# ice-box

**最高の sing-box GUI。**

macOS、Windows、Linux、Android 向けの軽量プロキシクライアント。<br>
導入が簡単で、起動が速く、実行中は静か、そして Agent フレンドリー — [sing-box](https://github.com/SagerNet/sing-box) コアをベースに構築されています。

[![Release](https://img.shields.io/github/v/release/yilong-musk/ice-box?style=flat-square&label=release)](https://github.com/yilong-musk/ice-box/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/yilong-musk/ice-box/ci.yml?branch=main&style=flat-square&label=CI)](https://github.com/yilong-musk/ice-box/actions/workflows/ci.yml)
[![Downloads](https://img.shields.io/github/downloads/yilong-musk/ice-box/total?style=flat-square)](https://github.com/yilong-musk/ice-box/releases)
[![Platforms](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux%20%7C%20Android-007ec6?style=flat-square)](#インストール)
[![Core](https://img.shields.io/badge/sing--box-1.14.4-6f42c1?style=flat-square)](https://github.com/SagerNet/sing-box)
[![License](https://img.shields.io/github/license/yilong-musk/ice-box?style=flat-square)](LICENSE)

[**ダウンロード**](https://github.com/yilong-musk/ice-box/releases/latest) · [**ライブデモ**](https://yilong-musk.github.io/ice-box/) · [インストール](#インストール) · [TUN](#tun-モード) · [ドキュメント](#ドキュメント) · [更新履歴](CHANGELOG.md)

[Bahasa Indonesia](README.id.md) · [Deutsch](README.de.md) · [English](README.md) · [Español](README.es.md) · [Français](README.fr.md) · [Português (BR)](README.pt-BR.md) · [Tiếng Việt](README.vi.md) · [Türkçe](README.tr.md)<br>[Русский](README.ru.md) · [Українська](README.uk.md) · [العربية](README.ar.md) · [فارسی](README.fa.md) · **日本語** · [한국어](README.ko.md) · [简体中文](README.zh-CN.md) · [繁體中文](README.zh-TW.md)

<br>

<a href="https://yilong-musk.github.io/ice-box/">
  <img src="docs/images/hero.png" alt="デスクトップ版および Android 版の ice-box：プロキシ状態、モード切り替え、出口ノード、リアルタイムトラフィックグラフ、ノード一覧を備えたモバイル向けホーム画面" width="880">
</a>

<sub><a href="https://yilong-musk.github.io/ice-box/">ライブデモ</a>では、デスクトップ版と Android 版の UI をシミュレートされたバックエンド上でブラウザから試せます。</sub>

</div>

## 機能

- **サブスクリプション。** sing-box JSON、Clash、共有リンク一覧を自動判別し、ローカルの sing-box 設定ファイルもインポートできます。デスクトップではプロファイルの自動更新が可能で、Android ではインポート時または手動更新時に再読み込みします。プロバイダーから提供されるトラフィック残量や有効期限も各サブスクリプションに表示されます。
- **ルーティング。** **ルール**、**グローバル**、**ダイレクト** の 3 モードを備え、サブスクリプション内の個別ルールをオン／オフできるほか、独自のカスタムルールも上に重ねられます。
- **ノード。** 戦略グループごとに出口を選択し、単体または一覧全体で遅延をテストでき、よく使うノードはお気に入りに固定できます。
- **接続。** デスクトップ（macOS、Windows、Linux）では、すべてのアクティブな接続（接続先、一致したルール、使用中の出口、累計転送量）をリアルタイムで確認でき、検索・並び替え、個別または一括での切断に対応します。
- **キャプチャ。** デスクトップではシステムプロキシまたは TUN を使用し、Android ではシステム VPN を使用してアプリ別ルーティング（ルールに従う、プロキシ、ダイレクト、ブロック）を設定できます。
- **トレイ。** ウィンドウを開かずにプロキシサービスの開始・停止や、モード・サブスクリプション・出口ノードの切り替えができます。macOS のメニューバー項目ではリアルタイムのトラフィックも表示され、メニューから遅延テストも実行できます。
- **Agent 連携。** オプトインのローカル MCP サービスにより、Agent が実行中の同じセッションを操作できます。
- **手間いらず。** ログイン時に自動起動、署名検証済みのアプリ内更新、ホームでのリアルタイム通信量とメモリ表示、完全な RTL サポートを含む 16 言語（Bahasa Indonesia、Deutsch、English、Español、Français、Português (BR)、Tiếng Việt、Türkçe、Русский、Українська、العربية、فارسی、日本語、한국어、简体中文、繁體中文）、ライトおよびダークテーマに対応しています。

## インストール

[最新リリース](https://github.com/yilong-musk/ice-box/releases/latest)からご使用のプラットフォーム向けインストーラーをダウンロードしてください。

| プラットフォーム | 動作要件 | インストーラー | 初回起動 |
|---|---|---|---|
| **macOS**（Apple Silicon） | macOS 12 以降 | `ice-box_<version>_aarch64.dmg` | ad-hoc 署名のみ（Developer ID なし、公証なし）のため、初回起動時に**開発元を確認できない**旨の警告が表示されます。**システム設定 → プライバシーとセキュリティ → このまま開く**（macOS 15+）で許可するか、右クリックから **開く**（それ以前のバージョン）を選択するか、`xattr -dr com.apple.quarantine /Applications/ice-box.app` を実行してください。 |
| **Windows**（x64） | Windows 10 以降 | `ice-box_<version>_x64-setup.exe` | ユーザー単位の NSIS インストーラーで、管理者権限は不要です。Authenticode 署名がないため、SmartScreen で確認を求められる場合があります。 |
| **Linux**（x64 / arm64） | Ubuntu 22.04+ などの主要ディストリビューション | `ice-box_<version>_amd64.AppImage` / `aarch64.AppImage`<br>`ice-box_<version>_amd64.deb` / `arm64.deb` | AppImage には実行権限を付与し（`chmod +x`）、`.deb` は `sudo apt install ./ice-box_*.deb` でインストールしてください。GNOME のトレイアイコンには AppIndicator 拡張機能（`gnome-shell-extension-appindicator`）が必要です。Wayland + NVIDIA 環境では、事前に設定されていない限り `WEBKIT_DISABLE_DMABUF_RENDERER=1` が自動的に設定されます。 |
| **Android**（arm64） | Android 14 以降 | `ice-box_<version>_android_arm64.apk` | プロジェクトのキーストアで署名されています。Google Play では配信していないため、初回インストール時にこの提供元からのインストール許可を求められます。 |

**アップデート。** macOS、Windows（0.1.5+）、Linux AppImage 版では、ice-box がバックグラウンドで GitHub Releases を確認し、**設定 → アプリの更新** から署名検証済みのアップデートをインストールします（それ以前の macOS/Windows 版では最初に一度手動でアップグレードしてください）。Linux の `.deb` 版も同様に更新カタログを確認し、`apt` / `dpkg` で管理できるようリリースのダウンロードページを開きます。Android では **設定 → アプリの更新** から arm64 APK をダウンロードし、システムのインストーラーに確認を引き継ぎます。

## クイックスタート

1. **サブスクリプションをインポートする。** **サブスクリプション** を開き、URL を貼り付けて **インポート** を押すと、形式が自動で判別されます。ローカルの sing-box 設定ファイル（JSON）を使う場合は **設定ファイル** を選択してください。リモート URL を持たないため更新はできません。
2. **モードとノードを選ぶ。** **ホーム** で **ルール**、**グローバル**、または **ダイレクト** を選びます。デスクトップでは **ノード** を開いて出口の選択と遅延テストを行い、Android では **ホーム** のノード一覧を使います。
3. **プロキシサービスを開始する。** 電源ボタンを押します。デスクトップではシステムプロキシが適用されるか、有効時に TUN が起動します。Android ではシステムの許可ダイアログ後に VPN が接続されます。

サブスクリプションをまだお持ちでなくても大丈夫です。ice-box はダイレクト専用モードで起動するため、インポート前にコア、キャプチャ方式、UI を試すことができます。

## ログイン時に自動起動

macOS、Windows、Linux では **設定 → 起動 → ログイン時に自動起動** を有効にしてください。ログイン時の自動起動ではウィンドウを表示せずトレイに常駐し、コアを起動して前回のプロキシ状態を復元します。

無効にする際も同じスイッチを使用してください。macOS の **ログイン項目** から無効にしてもアプリ内の設定は解除されません。また、macOS ではディスクイメージから直接起動すると拒否されるため、有効化する前にアプリをインストールしてください。Linux では `~/.config/autostart/com.yilong-musk.icebox.desktop` にエントリが書き込まれます（AppImage から実行した場合は `$APPIMAGE` を記録します）。

Android にはログイン項目がありません。**設定 → システム** でバッテリーの最適化からの除外、厳格なプライベート DNS、常時接続 VPN について案内しています。

## TUN モード

macOS、Windows、Linux では、TUN がネットワーク層でトラフィックをキャプチャするため、システムプロキシを参照しないアプリも対象に含められます。Windows の TUN は IPv4 TCP のみ対応（UDP/QUIC および IPv6 は非対応）で管理者アカウントが必要なため、その場合はシステムプロキシを代替手段として利用できます。Linux の TUN は Polkit（`pkexec`）を使って systemd の `ice-helper` デーモンをインストールし、`systemd-resolved`（`resolvectl`）または静的な `/etc/resolv.conf` 経由で DNS を管理します。設定方法、切り替え時の挙動、プラットフォーム対応状況、制限事項については [`docs/tun.md`](docs/tun.md) を参照してください。Android はシステム VPN を使用します（[`docs/architecture.md`](docs/architecture.md#clients) を参照）。

## MCP による Agent 連携

デスクトップアプリ（macOS、Windows、Linux）はオプトインのローカル MCP サービスを提供します。**接続** と **設定** の間にある **MCP** を開き、サービスを有効にして生成されたクライアント設定をコピーしてください。Agent は別のランタイムを起動せず、実行中セッションの設定、サブスクリプション、データ、コア、プロキシ状態を共有します。

- **包括的なツールセット。** 状態、設定、トラフィック、サブスクリプション、ノードとグループ、ルール、アクティブな接続、ログ、診断、プロキシ制御、保護された TUN 復旧を網羅します。可能な場合は結果に構造化コンテンツを含め、アプリのエラーコードも保持します。
- **デフォルトで安全。** MCP は認証済みのローカルループバックでのみ待機し、ランダムな制御トークンを使用します。このトークンがコピーしたクライアント設定に含まれることはありません。
- **内蔵 stdio ブリッジ。** 生成された設定は同じ実行ファイルをブリッジモードで起動するため、別のデーモンやプロトコルアダプターは不要です。

MCP はデスクトップ専用です。Android には MCP がなく、Web サイトのデモではシミュレートされたサービスを使用します。セットアップ、ライフサイクル、セキュリティ境界の詳細は [`docs/mcp.md`](docs/mcp.md) を参照してください。

## 開発

Rust（`rust-toolchain.toml` に固定されたバージョン。初回実行時に rustup が自動インストール）、Node.js 22、macOS では Xcode Command Line Tools、Linux では GTK/WebKit 開発パッケージ（`libwebkit2gtk-4.1-dev`、`libgtk-3-dev`、`libayatana-appindicator3-dev`、`librsvg2-dev`、`libssl-dev`）が必要です。Android のビルドにはさらに JDK 17、Go、および `scripts/ci-install-android-sdk.sh` で必要なパッケージをインストールした Android SDK（`ANDROID_HOME`）が必要です。

```bash
cd apps/desktop && npm install && cd ../..
npm run fetch-singbox   # このホスト向けに固定バージョンの sing-box コアを取得
npm run dev
```

| コマンド | 用途 |
|---|---|
| `npm run dev:mac-arm64` / `dev:mac-x64` / `dev:win` / `dev:linux-x64` / `dev:linux-arm64` | 指定したホストターゲット向けに実行 |
| `npm run gate` | ワークスペース全体のゲート：fmt、clippy、Rust テスト、型チェック、ビルド、スモークテスト、fixtures |
| `npm run fetch-singbox && npm run build` | macOS `.dmg` をビルド |
| `npm run fetch-singbox -- win && npm run build:win` | Windows NSIS インストーラーをビルド |
| `npm run fetch-singbox -- linux-x64 && npm run build:linux` | Linux `.deb` および `.AppImage` をビルド（arm64 では `linux-arm64` を指定） |
| `npm run acceptance` / `acceptance:tun` / `acceptance:win` / `acceptance:linux` / `acceptance:linux-tun` | 実ホストでの受け入れゲートテスト |
| `bash scripts/build-android-apk.sh debug arm64` | arm64 実機向けデバッグ APK をビルド |
| `bash scripts/build-android-apk.sh debug amd64` | エミュレーター向けデバッグ APK をビルド |

```
apps/desktop    macOS、Windows、Linux 向け React UI + Tauri シェル
apps/mobile     Android シェル（デスクトップ UI を再利用）
apps/website    GitHub Pages サイトおよびライブデモ
crates/         Rust ワークスペース（主な crate）：ice-app, ice-config, ice-core, ice-engine, ice-subscription, ice-proxy-sys, ice-tun-sys, ice-elevate, ice-helper, ice-types, ...
```

## ドキュメント

| ドキュメント | 内容 |
|---|---|
| [`docs/architecture.md`](docs/architecture.md) | システム構成、コンポーネントの責務、データフロー、設計境界 |
| [`docs/mcp.md`](docs/mcp.md) | デスクトップ MCP のセットアップ、共有ランタイムモデル、ツール、セキュリティ |
| [`docs/tun.md`](docs/tun.md) | TUN の挙動、復旧機構、プラットフォーム設定、制限事項 |
| [`docs/testing.md`](docs/testing.md) | ローカルおよび CI のゲートカバレッジ、手動実機テスト |
| [`docs/release-process.md`](docs/release-process.md) | バージョニング、署名、パッケージング、公開手順 |
| [`CHANGELOG.md`](CHANGELOG.md) | リリース履歴 |

上記の技術ドキュメントは現在英語のみで提供されています。

## ライセンス

ice-box は自由ソフトウェアであり、[GNU 一般公衆利用許諾書 v3.0 以降](LICENSE) のもとで公開されています。同梱されている sing-box コアも [GPL-3.0-or-later](third_party/sing-box/LICENSE) であり、アップストリームの命名制限が適用されます。すべてのサードパーティライセンス表記は [NOTICE](NOTICE) を参照してください。

<div align="center">
<br>
<sub>Tauri、React、sing-box で構築されています。</sub>
</div>
