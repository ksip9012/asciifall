# asciifall

マトリックス映画風の落下文字アニメーション。macOS の zsh ターミナルで動作する Rust 製バイナリ。

## ビルドと実行

```zsh
# mise で Rust（mise.toml の版と clippy・rustfmt）と pre-commit を入れる（初回のみ）
mise trust
mise install
pre-commit install

# format・clippy・テスト（CI・pre-commit と同じ）
mise run check

# 開発ビルド（動作確認）
cargo run

# リリースビルド（最適化済み）
cargo build --release
./target/release/asciifall
```

## 依存クレート

- `crossterm` — ターミナル制御（raw mode・ANSI カラー・カーソル・リサイズイベント）
- `rand` — 落下パラメータのランダム生成

## プロジェクト構成

```
src/main.rs   — 全実装（単一ファイル、~350行）
```

## 主要な概念

- **Column** — ターミナル幅に対応する縦の列
- **Drop** — 列内を落下する文字ストリーム
- **Tick** — 1アニメーションステップ（33ms = 30fps 目標）
- **speed** — Drop が N tick ごとに1行進む速度

## 操作

- `q` または `Ctrl+C` — 終了してターミナルを元の状態に戻す

## 動作要件

- RGB カラー対応ターミナル（iTerm2、macOS Terminal.app 10.15+）
- 代替スクリーンバッファ対応（現代的なターミナルなら問題なし）
- 等幅フォント
