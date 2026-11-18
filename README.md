# ViewIP

LAN 内の機器を一覧表示するシンプルな ping スキャナ（Windows 用）。
`x.x.x.0` 〜 `x.x.x.255` を 4 列 × 64 行で表示し、応答した機器を緑色でホスト名とともに表示します。

IgPing と同じ使い勝手で、**スキャンに使うネットワークアダプタを選べる**のが特徴です。
Wi-Fi と有線 LAN に同時につながっていても、有線側のネットワークだけを調べられます。

## 使い方

1. [Releases](../../releases) から `viewip.exe` をダウンロードして起動（インストール不要）
2. 画面下のプルダウンでアダプタを選ぶ
   - 前回の選択を記憶します。初回は有線 LAN を優先して選びます

動作環境: Windows 10 / 11（WebView2 が必要。Windows 11 には標準で入っています）

## 仕組み

- Windows API `IcmpSendEcho2Ex` に送信元 IP を指定して ping を送るため、選んだアダプタから送信されます（管理者権限不要）
- 1〜254 に並列で ping し、1.5 秒ごとに更新。2 回連続で応答がなければ消えます
- ホスト名は Windows の逆引き（DNS / NetBIOS / LLMNR）で取得
- スキャン範囲は選んだアダプタの IP の /24

## ビルド

必要なもの: Rust (stable, MSVC), Node.js

```
npm install
npm run build
```

`src-tauri/target/release/viewip.exe` が生成されます。

開発時は `npm run dev`。実ネットワークでの動作確認テスト:

```
cd src-tauri
cargo test -- --ignored --nocapture
```

## 構成

- `src-tauri/src/net.rs` — アダプタ一覧取得、ping、ホスト名逆引き
- `src-tauri/src/main.rs` — スキャンループ、Tauri コマンド
- `src/` — 画面（HTML / CSS / JS、ビルドツールなし）
