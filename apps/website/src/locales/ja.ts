// SPDX-License-Identifier: GPL-3.0-or-later

import type { MarketingKey } from "./zh";

export const ja: Record<MarketingKey, string> = {
  "head.title": "ice-box — 最高の sing-box GUI",
  "head.description":
    "ice-box は macOS、Windows、Linux、Android 向けの軽量プロキシクライアントです。導入が簡単で起動が速く、省リソースで動作し、MCP による Agent 連携にも対応します。",
  "nav.languageAria": "言語を選択",
  "nav.themeToLight": "ライトモードに切り替え",
  "nav.themeToDark": "ダークモードに切り替え",
  "nav.features": "機能",
  "nav.agent": "Agent",
  "nav.download": "ダウンロード",
  "hero.release": "{version} リリース",
  "hero.title": "最高の <span class=\"accent\">sing-box GUI</span>",
  "hero.subtitle":
    "macOS、Windows、Linux、Android 向けの軽量プロキシクライアント。サブスクリプションを読み込み、ノードを選んでワンクリックで開始。バックグラウンドでは静かに動作し、MCP 経由で Agent とも協働できます。",
  "hero.installer": "無料ダウンロード",
  "hero.installerPlatform": "{platform} 版をダウンロード",
  "hero.tryDemo": "ライブデモを試す",
  "hero.platforms": "macOS · Windows · Linux · Android",
  "demo.windowBar": "ライブ製品デモ",
  "demo.iframeTitle": "ice-box デスクトップ版のライブデモ",
  "features.kicker": "機能",
  "features.title": "毎日使う機能を、すぐ手の届く場所に",
  "features.body":
    "サブスクリプションのインポートから接続の確認まで、日常の操作はすべて 1〜2 クリックで完結します。",
  "features.groupDaily": "毎日の操作",
  "features.groupBackground": "バックグラウンド",
  "features.subs.title": "サブスクリプション",
  "features.subs.body":
    "sing-box JSON、Clash、共有リンクを自動判別し、ローカル設定にも対応。デスクトップでは自動更新でき、残容量や有効期限も一目で確認できます。",
  "features.routing.title": "ルーティング",
  "features.routing.body":
    "ルール・グローバル・ダイレクトの 3 モードをいつでも切り替え。サブスクリプション内の個別ルールをオン／オフでき、カスタムルールも重ねられます。",
  "features.nodes.title": "ノード",
  "features.nodes.body":
    "グループごとに出口を選択し、単体または一括で遅延をテスト。よく使うノードはお気に入りに固定できます。",
  "features.connections.title": "接続",
  "features.connections.body":
    "デスクトップ（macOS、Windows、Linux）でアクティブな接続の宛先、一致ルール、出口、トラフィックを確認。検索・並び替え・一括切断に対応します。",
  "features.capture.title": "キャプチャ方式",
  "features.capture.body":
    "デスクトップはシステムプロキシまたは TUN を選択可能。Android ではシステム VPN として動作し、アプリごとにルール準拠・プロキシ・ダイレクト・ブロックを設定できます。",
  "features.tray.title": "トレイ",
  "features.tray.body":
    "ウィンドウを開かずにプロキシの開始／停止やモード・サブスクリプション・出口ノードの切り替えが可能。macOS メニューバーにはリアルタイム速度も表示されます。",
  "features.upkeep.title": "手間いらず",
  "features.upkeep.body":
    "ログイン時自動起動、署名検証済みのアプリ内更新、ホームでのリアルタイム通信量・メモリ表示、RTL を含む 16 言語とライト／ダークテーマに対応。",
  "agent.kicker": "Agent フレンドリー",
  "agent.title": "Agent と同じセッションを共有する",
  "agent.body":
    "デスクトップ版にはオプションのローカル MCP サービスを内蔵しています。有効にして設定を Agent に渡せば、状態の取得、モードやノードの切り替え、サブスクリプションやルール・接続の管理まで、GUI とまったく同じデータに対して操作できます。",
  "agent.point1": "余計な常駐プロセスやデータフォルダの複製は不要",
  "agent.point2": "あなたと Agent の操作が互いにリアルタイムで反映",
  "agent.point3": "デフォルトは無効、ローカルループバックのみ待機",
  "agent.terminalTitle": "Agent セッション",
  "agent.prompt": "グローバルモードに切り替えて、通信量が多い接続を教えて",
  "agent.reply":
    "グローバルモードに切り替えました。8 件の接続のうち通信量が最大なのは googlevideo.com（YouTube）で、Los Angeles / edge-03 を経由しています。",
  "phoneDemo.iframeTitle": "ice-box Android 版のライブデモ",
  "download.kicker": "ダウンロード",
  "download.title": "プラットフォームを選択",
  "download.body": "インストーラーは GitHub Releases で配布しています。アカウント登録は不要です。",
  "download.tabListAria": "OS を選択",
  "download.actionPlatform": "{platform} 版をダウンロード",
  "download.yours": "この端末",
  "download.allReleases": "すべてのリリースと更新履歴を見る",
  "download.macos.name": "macOS",
  "download.macos.kind": "Apple Silicon",
  "download.macos.requires": "macOS 12 以降",
  "download.macos.note":
    "初回起動時に「開発元を確認できない」と表示された場合は、システム設定 → プライバシーとセキュリティ から開くのを許可してください。",
  "download.windows.name": "Windows",
  "download.windows.kind": "64 ビット",
  "download.windows.requires": "Windows 10 以降",
  "download.windows.note":
    "現在のユーザー向けに管理者権限なしでインストールされます。SmartScreen の確認が表示される場合があります。",
  "download.linux.name": "Linux",
  "download.linux.requires": "Ubuntu 22.04+ などの主要ディストリビューション",
  "download.linux.note":
    "AppImage は署名済みアプリ内更新に対応し、.deb はパッケージマネージャーで更新します。システムプロキシは GNOME/KDE 対応、TUN には systemd と Polkit が必要です。",
  "download.linux.archAria": "Linux アーキテクチャを選択",
  "download.linux.appimageAction": "AppImage をダウンロード",
  "download.linux.debAction": ".deb をダウンロード",
  "download.android.name": "Android",
  "download.android.kind": "arm64",
  "download.android.requires": "Android 14 以降",
  "download.android.note":
    "アプリストアでは未配信です。インストール時に Android から確認が出たら、この提供元からのインストールを許可してください。",
  "download.cli.subtitle":
    "macOS、Windows、Linux 向けのヘッドレス CLI とデーモン。Agent、サーバー、スクリプト自動化に対応します。",
  "download.cli.shellAria": "CLI インストールコマンドを選択",
  "download.cli.copy": "コピー",
  "download.cli.copied": "コピー済み",
  "download.cli.archives": "アーカイブ直接ダウンロード",
  "download.cli.docs": "CLI ドキュメント",
  "footer.source": "ソースコード",
  "footer.releases": "リリース",
  "footer.changelog": "更新履歴",
  "footer.license": "GPL-3.0-or-later ライセンス",
};
