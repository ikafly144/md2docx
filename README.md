# mdd

[nokonoko1203/md2docx](https://github.com/nokonoko1203/md2docx) からのフォーク。CSSによるスタイル適用やページ設定の拡張など、業務用途向けの機能を追加している。

MarkdownファイルをWord(.docx)に変換するCLIツール。

![alt text](images/README/image.png)

## 対応するMarkdown要素

| 要素             | 記法                | 変換時の処理                              |
| ---------------- | ------------------- | ----------------------------------------- |
| 見出し           | `# H1` ～ `##### H5` | 自動採番付き。H1/H2はWord上で前後に間隔を付与 |
| 段落             | 通常のテキスト      | 字下げ・余白をtwip単位で制御              |
| 箇条書き         | `- item`            | ネスト対応、行頭文字カスタマイズ可（●■▲） |
| 番号付きリスト   | `1. item`           | ネスト対応、開始番号指定可                |
| 表               | GFM形式             | 自動採番（連番 or 章番号付き、設定で切替） |
| 画像             | `![alt](path)`      | 自動採番（連番 or 章番号付き）、形式自動変換 |
| コードブロック   | ` ``` `             | Courier New / MS ゴシック、9pt（CSSで変更可） |
| 引用             | `> text`            | 左ボーダー・インデント付き（CSSで変更可） |
| 改ページ         | `\pagebreak`        | Wordの改ページを挿入                      |
| 水平線           | `---`               | 空段落に変換                              |
| 太字             | `**text**`          | 対応                                      |
| 斜体             | `*text*`            | 対応                                      |
| インラインコード | `` `code` ``        | 「」で囲んで表示                          |
| リンク           | `[text](url)`       | ハイパーリンク化、アンカーリンクにも対応  |
| span装飾         | `<span class="x">` | CSSクラスによる文字装飾（`--css`指定時）  |
| div装飾          | `<div class="x">`  | CSSクラスによるブロック装飾（`--css`指定時） |

## pandocとの違い

pandocは汎用の文書変換ツールだが、mddは日本語のWord文書を作ることに特化している。

- **日本語フォントがデフォルト**。本文は游明朝、見出しは游ゴシック。英語フォントも別途指定でき、混植が自然になる。
- **見出しの自動採番**。H1からH5まで階層的に番号を振る（1 → 1.1 → 1.1.1 → (1) → ①）。既存の番号があれば重複を避ける。
- **表・画像の自動採番**。連番（図1, 図2…）と章番号付き（図1.1, 図1.2, 図2.1…）を設定で切り替えられる。章番号付きではH1の番号を基準にし、H1が変わるとリセットされる。
- **TOML設定ファイル一つで制御**。フォント、サイズ、ページ設定、インデント、行頭文字をまとめて指定できる。pandocのようにテンプレートdocxを用意する必要がない。
- **英日間スペースの自動削除**。日本語と英語の境界にある不要なスペースを消して、組版を整える。
- **シングルバイナリ**。Rustで書かれておりLaTeXやPython環境が不要。`cargo install`だけで使える。

## このフォークで追加された機能

- **`--css` オプション** — CSSファイルで見出し・テーブルヘッダ・span装飾・blockquote・コードブロック・div装飾のスタイルを指定できる。詳細は[CSSスタイル](#cssスタイル)セクションを参照。
- **`<span class="...">` 装飾** — Markdown内のHTML spanタグにCSSクラスのスタイルを適用する。
- **`<div class="...">` 装飾** — Markdown内のHTML divタグにCSSクラスのブロックスタイルを適用する。
- **引用（blockquote）のスタイル** — デフォルトで左ボーダー・グレー色・インデント付き。CSSで `blockquote` セレクタを指定してカスタマイズ可能。
- **コードブロックのCSS対応** — `pre`/`code` セレクタでフォント・サイズ・背景色・ボーダーを指定できる。
- **`image_max_height` 設定** — 縦長画像の高さ上限をtwip単位で指定できる（`[page]` セクション）。詳細は[設定ファイル](#設定ファイル)を参照。
- **`heading_numbering` 設定** — 見出しの自動採番を無効化できる（`[numbering]` セクション）。
- **`[toc]` セクション** — 目次（Table of Contents）の自動生成。見出しレベルの範囲指定も可能。Wordで開いた後にフィールド更新（Ctrl+A → F9）でページ番号が反映される。
- **`[spacing]` セクション** — TOML設定ファイルで行間・段落前後の間隔をpt単位で一括指定できる。

## インストール

Rust 1.70以上が必要。

コマンドとしてインストールする場合は以下を実行する。

```sh
cargo install --path .
```

$HOME/.cargo/bin/mddにバイナリが入る。PATHが通っていなければ、~/.zshrcなどに次の行を足す。

```sh
export PATH="$HOME/.cargo/bin:$PATH"
```

ビルドだけしたい場合はこちら。

```sh
cargo build --release
```

バイナリはtarget/release/mddに生成される。

## 使い方

```sh
mdd <入力ファイル> [オプション]
```

mdd --helpで詳細を確認できる。

| オプション      | 説明                                                                              |
| --------------- | --------------------------------------------------------------------------------- |
| `-o, --output`  | 出力先を指定する。省略すると入力ファイル名の拡張子を `.docx` に変えたものになる。 |
| `-c, --config`  | 設定ファイル (TOML) を指定する。省略するとデフォルト設定が使われる。              |
| `--css`         | CSSファイルを指定する。見出し・テーブルヘッダ・span・div・blockquote・コードブロックのスタイルを適用する。 |
| `-h, --help`    | ヘルプを表示する。`--help` なら設定ファイルの書式も出る。                         |
| `-V, --version` | バージョンを確認する。                                                            |

```sh
# 基本的な変換（document.docxが生成される）
mdd document.md

# 出力先を指定
mdd document.md -o output.docx

# 設定ファイルを指定
mdd document.md -o output.docx -c mdd.toml

# CSSスタイルを適用
mdd document.md --css styles.css
```

## 設定ファイル

TOML形式でフォントやサイズをカスタマイズできる。すべての項目は省略可能で、省略した項目にはデフォルト値が入る。全項目とデフォルト値はmdd --helpで確認できる。

```toml
[fonts]
body_ja = "游明朝"
body_en = "Century"
heading_ja = "游ゴシック"
heading_en = "Century"

[sizes]
body = 10.5
heading1 = 14.0
heading2 = 12.0
heading3 = 11.0
heading4 = 11.0
heading5 = 10.5

[page]
width = 11906
height = 16838
margin_top = 1985
margin_right = 1701
margin_bottom = 1701
margin_left = 1701
margin_header = 851
margin_footer = 992
margin_gutter = 0
# image_max_height = 8505  # 縦長画像の高さ上限（twip単位、約15cm。省略時は制限なし）

[indent]
body_left = 210
body_first_line = 210
body_right = 210
body_left_chars = 100
heading4_left = 709
heading4_hanging = 709

[bullet]
level0 = "●"
level1 = "■"
level2 = "▲"

[numbering]
heading_numbering = true     # 見出しの自動採番（false で無効化、省略時: true）
h1_title = false             # H1をタイトル扱いにし採番しない（省略時: false）
heading_numbering_depth = 5  # 自動採番の対象レベル深さ（省略時: 5）
figure_format = "chapter"    # 図番号の形式: "sequential"（連番）/ "chapter"（章番号付き）（省略時: "sequential"）
table_format = "chapter"     # 表番号の形式: "sequential" / "chapter"（省略時: "sequential"）

[toc]
enable    = false    # 目次の生成（省略時: false）
min_level = 1        # 目次に含める最小見出しレベル
max_level = 3        # 目次に含める最大見出しレベル

[spacing]
# line = 18.0       # 行間（pt単位、省略時: Word既定）
# before = 6.0      # 段落前の間隔（pt単位、省略時: なし）
# after = 6.0       # 段落後の間隔（pt単位、省略時: なし）
```

fontsセクションでは本文と見出しそれぞれの日本語フォント、英語フォントを指定する。sizesセクションで本文と各レベルの見出し（H1〜H5）のフォントサイズをpt単位で設定する。

pageセクションではページサイズと余白をtwip単位で設定する。既定値はA4縦相当で、画像の最大幅もこのページ幅と左右余白から自動計算される。そのため、横長の画像でも本文幅に収まる。`image_max_height` を指定すると、縦長画像の高さにも上限を設けられる（twip単位、省略時は制限なし）。幅・高さともにアスペクト比を維持してスケーリングされる。レイアウトを変えたい場合は `width` と `margin_left` / `margin_right` を主に調整すればよい。

indentセクションのtwipはWordの内部単位で、1twipは1/20pt。210twipがおおむね全角1文字分にあたる。body_left_charsはWord独自の文字数単位で、100が1文字に相当する。

bulletセクションで箇条書きの各レベルに使う行頭文字を変更できる。

numberingセクションで見出しの自動採番と図番号・表番号の採番形式を指定する。`heading_numbering = false` にすると見出しの自動採番を無効化できる。`h1_title = true` にするとH1をタイトル扱いにし採番を外す（H2以下の番号レベルが1段繰り上がる）。`heading_numbering_depth` で自動採番の対象レベル深さを制御できる（既定: 5、H5まで）。`figure_format` / `table_format` は `"sequential"`（連番: 図1, 図2, 図3…）または `"chapter"`（章番号付き: 図1.1, 図1.2, 図2.1…）を指定する。章番号はH1（見出し1）の番号を基準とし、H1が変わるとリセットされる。H2以下の変化ではリセットされない。

tocセクションで目次（Table of Contents）の生成を制御する。`enable = true` にするとドキュメント先頭に目次を挿入する。`min_level` と `max_level` で目次に含める見出しレベルの範囲を指定できる。生成されたdocxをWordで開いた時に「フィールドの更新」（Ctrl+A → F9）を実行すると目次のページ番号が正しく表示される。

spacingセクションで本文の行間と段落前後の間隔をpt単位で指定できる。すべて省略可能で、省略した項目はWordの既定値が使われる。

## CSSスタイル

`--css` オプションでCSSファイルを指定すると、見出し・テーブルヘッダ・インラインspan・blockquote・コードブロック・div装飾にスタイルを適用できる。md-to-pdfなど他のツールと共通のCSSでPDFとDOCXの見た目を一元管理する用途を想定している。

### 対応セレクタ

| セレクタ         | 対象                                        |
| ---------------- | ------------------------------------------- |
| `h1`〜`h5`      | 見出し1〜5の段落スタイル                     |
| `table th`       | テーブルヘッダセル                           |
| `pre` / `code`   | コードブロックの段落スタイル                 |
| `blockquote`     | 引用ブロックの段落スタイル                   |
| `.クラス名`      | `<span>` の文字装飾 / `<div>` のブロック装飾 |

上記以外のセレクタ（IDセレクタ、子孫セレクタ、タグ+classセレクタなど）は警告を出して無視する。

### 対応プロパティ

| CSSプロパティ      | 受理する値                      | DOCXへのマッピング                        |
| ------------------ | ------------------------------- | ----------------------------------------- |
| `color`            | `#rrggbb`                       | フォント色                                |
| `background-color` | `#rrggbb`                       | セル/段落背景色                            |
| `font-weight`      | `bold` / `normal`               | 太字の設定・解除                          |
| `font-style`       | `italic` / `normal`             | 斜体の設定・解除                          |
| `text-decoration`  | `underline` / `none`            | 下線の設定・解除                          |
| `font-family`      | 文字列（引用符可）              | フォント指定                              |
| `font-size`        | `Npt`（pt単位のみ）             | フォントサイズ                            |
| `line-height`      | `Npt`（pt単位のみ）             | 行間（h1〜h5のみ）                        |
| `border`           | `Npx style #rrggbb`            | 段落ボーダー                              |
| `border-top`       | `Npx style #rrggbb`            | 段落上ボーダー                            |
| `border-bottom`    | `Npx style #rrggbb`            | 段落下ボーダー                            |
| `border-left`      | `Npx style #rrggbb`            | 段落左ボーダー                            |
| `border-right`     | `Npx style #rrggbb`            | 段落右ボーダー                            |
| `padding`          | `Npt`（pt単位のみ）             | 上下左右のパディング一括指定              |
| `padding-top`      | `Npt`（pt単位のみ）             | 段落前の間隔                              |
| `padding-right`    | `Npt`（pt単位のみ）             | 右インデント                              |
| `padding-bottom`   | `Npt`（pt単位のみ）             | 段落後の間隔                              |
| `padding-left`     | `Npt`（pt単位のみ）             | 左インデント                              |

`rgb()`、`hsl()`、`em`/`rem`単位、CSS変数、`calc()`、`inherit`、`!important` などは非対応で、警告を出して無視する。

### CSSの例

```css
h1 {
  font-size: 28pt;
  line-height: 36pt;
}

h2 {
  font-size: 22pt;
  line-height: 30pt;
}

h3 {
  font-size: 18pt;
}

table th {
  background-color: #eeeeee;
  font-weight: bold;
}

.warning {
  color: #c00000;
  font-weight: bold;
}

blockquote {
  color: #666666;
  border-left: 3px solid #cccccc;
  padding-left: 12pt;
}

pre, code {
  font-family: "Courier New", monospace;
  font-size: 9pt;
  background-color: #f5f5f5;
  border-left: 3px solid #dddddd;
}

.todo {
  background-color: #fff3cd;
  border-left: 4px solid #ffc107;
  color: #856404;
  padding-left: 10pt;
}
```

### Markdownでのspan記法

Markdown内で `<span class="クラス名">テキスト</span>` と書くと、CSSで定義したクラスのスタイルが適用される。

```markdown
通常のテキスト。<span class="warning">この部分が赤太字になる</span>。
```

CSSに定義されていないクラス名を使った場合は警告を出すが、変換は継続する。

### Markdownでのdiv記法

Markdown内で `<div class="クラス名">` と `</div>` で囲んだブロックに、CSSクラスのスタイルを適用できる。`<div>` タグは単独行に記述する必要がある（pulldown-cmarkのブロックHTML制約）。

```markdown
<div class="todo">

ここにTODO項目を書く。背景色・左ボーダーが適用される。

- アイテム1
- アイテム2

</div>
```

### 制限事項

- コードブロックは行ごとに独立段落なので、背景色・ボーダーも行ごとに適用される
- `<div>` タグは単独行に記述する必要がある（pulldown-cmarkのブロックHTML制約）
- フォントスタック（`"Courier New", Courier, monospace`）は最初のフォントのみ使用される
- `padding-top`/`padding-bottom` は段落前後の間隔（spacing before/after）で近似される
- `margin`, `border-radius` は非対応（Wordの段落モデルの制約）

見出しはH1からH5まで対応しており、自動採番が付く。H1とH2にはWord上で前後の段落間隔を設定し、章や節の区切りが詰まりすぎないようにしている。段落、箇条書き、番号付きリストはネストにも対応する。表には自動で表番号が振られ、画像には図番号が付く。採番形式は設定で連番・章番号付きを切り替えられる。コードブロック、改ページ、水平線にも対応している。インライン要素としてはテキスト、コード、太字、斜体、リンクを扱える。

## 改ページ

段落単位で`\pagebreak`のみを書いた場合、その位置にWordの改ページを挿入する。

```md
# 1章
本文

\pagebreak

# 2章
次のページから始まる本文
```

`\pagebreak`を通常段落や見出し、コードブロックなどに混在させた場合は、曖昧な解釈を避けるためエラーにする。
