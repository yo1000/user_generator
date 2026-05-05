dummy-user-generator
================================================================================

CLI tool for rapidly generating random lists of Japanese users.
Supports multi-threading parallel generation & parallel CSV writing using Rayon.

> [!NOTE]
> This software is based on AI-generated code with additional implementation.


Quickstart
--------------------------------------------------------------------------------

```bash
mkdir -p /tmp/out
docker run --rm \
-v /tmp/out:/var/output \
-e USERGEN__COUNT=10000 \
ghcr.io/yo1000/dummy-user-generator:latest

ls -l /tmp/out
less /tmp/out/users.csv
```


Build and Run
--------------------------------------------------------------------------------

```bash
# Build
cargo build --release

# Run
./target/release/usergen --count 100000
```


Docker build and Run
--------------------------------------------------------------------------------

```bash
# Build
cargo build --release
docker build --tag dummy-user-generator \
--build-arg BIN=./target/release/usergen \
--build-arg DATA_DIR=./data .

# Run
mkdir -p /tmp/out
docker run --rm \
-v /tmp/out:/var/output \
-e USERGEN__COUNT=10000 \
dummy-user-generator
```


--------------------------------------------------------------------------------


dummy-user-generator
================================================================================

ランダムな日本人ユーザーリストを高速生成する CLI ツールです。
Rayon による **マルチスレッド並列生成 & 並列 CSV 書き込み** に対応しています。


ディレクトリ構成
--------------------------------------------------------------------------------

```
dummy-user-generator/
├── Cargo.toml
├── src/
│   └── main.rs
└── data/
    ├── family_name.csv
    ├── given_name_female.csv
    ├── given_name_male.csv
    ├── ken_frequency.csv
    └── utf_ken_all.zip
```


入力ファイルの仕様
--------------------------------------------------------------------------------

### `family_name.csv`

| 列 | 内容        |
|----|-----------|
| 0  | 姓（漢字）     |
| 1  | 姓（ひらがな）   |
| 2  | 姓（ローマ字）   |
| 3  | 出現頻度（1〜3） |

ヘッダー行 `kanji,hiragana,romaji,frequency` が必要です。

出現頻度の重みは `1 / 8^(frequency - 1)` で計算されます。

| frequency | 重み       |
|-----------|----------|
| 1         | 1.0      |
| 2         | 0.125    |
| 3         | 0.015625 |

### `given_name_male.csv` / `given_name_female.csv`

| 列 | 内容        |
|----|-----------|
| 0  | 名（漢字）     |
| 1  | 名（ひらがな）   |
| 2  | 名（ローマ字）   |
| 3  | 出現頻度（1〜3） |

ヘッダー行 `kanji,hiragana,romaji,frequency` が必要です。

出現頻度の重みは姓と同じく `1 / 8^(frequency - 1)` で計算されます。

### `ken_frequency.csv`

| 列 | 内容           |
|---|--------------|
| 0 | 都道府県名（漢字）    |
| 1 | 都道府県名（ひらがな）  |
| 2 | 都道府県名（ローマ字）  |
| 3 | 人口           |
| 4 | 出現頻度（1〜10）   |

ヘッダー行 `prefecture,hiragana,romaji,population,frequency` が必要です。

出現頻度の重みは `1 / 2^(frequency - 1)` で計算されます。

| frequency | 重み     |
|-----------|--------|
| 1         | 1.0    |
| 2         | 0.5    |
| 3         | 0.25   |
| 4         | 0.125  |
| 5         | 0.0625 |

このファイルに載っていない都道府県は重み `1.0` として扱われます。

### `utf_ken_all.zip`

日本郵便が配布している郵便番号データ（UTF-8 エンコード）を ZIP 圧縮したファイル。
ダウンロード先: https://www.post.japanpost.jp/zipcode/dl/utf/zip/utf_ken_all.zip

ZIP 内の CSV の各列：

| 列インデックス | 内容        |
|---------------|-----------|
| 2             | 7桁郵便番号    |
| 6             | 都道府県名（漢字） |
| 7             | 市区町村名（漢字） |
| 8             | 町域名（漢字）   |


ビルド
--------------------------------------------------------------------------------

```bash
# リリースビルド（最速）
cargo build --release

# バイナリは target/release/usergen に生成される
```


使い方
--------------------------------------------------------------------------------

```bash
# 基本（デフォルト: 1000件、data/ ディレクトリのファイルを使用）
./target/release/usergen

# 件数を指定
./target/release/usergen --count 1000000

# ファイルパスをすべて明示指定
./target/release/usergen \
  --count             5000000 \
  --family-name       data/family_name.csv \
  --given-name-male   data/given_name_male.csv \
  --given-name-female data/given_name_female.csv \
  --ken-frequency     data/ken_frequency.csv \
  --ken-all           data/utf_ken_all.zip \
  --output-dir        ./output \
  --chunk-size        1000000 \
  --threads           8
```

### オプション一覧

| オプション                 | 短縮   | デフォルト                        | 説明                   |
|-----------------------|------|------------------------------|----------------------|
| `--count`             | `-c` | `1000`                       | 生成件数（最大 10,000,000）  |
| `--family-name`       |      | `data/family_name.csv`       | 姓 CSV パス             |
| `--given-name-male`   |      | `data/given_name_male.csv`   | 男性名 CSV パス           |
| `--given-name-female` |      | `data/given_name_female.csv` | 女性名 CSV パス           |
| `--ken-frequency`     |      | `data/ken_frequency.csv`     | 都道府県出現頻度 CSV パス      |
| `--ken-all`           |      | `data/utf_ken_all.zip`       | 郵便番号 ZIP パス          |
| `--output-dir`        | `-o` | `output`                     | 出力ディレクトリ             |
| `--chunk-size`        |      | `1,000,000`                  | ファイル分割単位             |
| `--threads`           |      | `0`（全コア）                     | 使用スレッド数              |


出力ファイル
--------------------------------------------------------------------------------

- **1 チャンクの場合** → `output/users.csv`
- **複数チャンクの場合** → `output/users_0001.csv`, `output/users_0002.csv`, ...

### CSV 列定義

| 列                    | 内容                                         |
|----------------------|--------------------------------------------|
| `username`           | 姓ローマ字.名ローマ字（小文字）、重複時は連番付き                 |
| `email`              | `username@example.com`                     |
| `familyName`         | 姓（漢字）                                      |
| `familyNameHiragana` | 姓（ひらがな）                                    |
| `familyNameRomaji`   | 姓（ローマ字）                                    |
| `givenName`          | 名（漢字）                                      |
| `givenNameHiragana`  | 名（ひらがな）                                    |
| `givenNameRomaji`    | 名（ローマ字）                                    |
| `gender`             | 1=男性、2=女性                                  |
| `birthDate`          | 生年月日（YYYY-MM-DD）、12〜105歳                   |
| `phoneNumber`        | 電話番号（例: 090-0123-4567）、重複なし                |
| `postcode`           | 郵便番号（例: 100-0001）                          |
| `prefectureName`     | 都道府県名                                      |
| `municipalityName`   | 市区町村名                                      |
| `townAreaName`       | 町域名（後述の加工ルールを適用）                           |


アーキテクチャ
--------------------------------------------------------------------------------

```
main()
  ├─ CSV/ZIP 読み込み（シングルスレッド）
  │    ├─ family_name.csv       → WeightedTable<FamilyName>
  │    ├─ given_name_male.csv   → WeightedTable<GivenName>
  │    ├─ given_name_female.csv → WeightedTable<GivenName>
  │    ├─ ken_frequency.csv     → HashMap<都道府県名, 重み>
  │    └─ utf_ken_all.zip       → WeightedTable<Vec<Address>>
  │                                  ├─ 第1段階: 都道府県を重み付き選択
  │                                  └─ 第2段階: 都道府県内の住所を均等選択
  │
  ├─ 電話番号インデックス事前生成（シングルスレッド）
  │    └─ Fisher-Yates partial shuffle → Arc<Vec<u32>>（全件・重複なし）
  │
  └─ rayon::par_iter（チャンク単位で並列）
       ├─ Thread 0: generate N users → write users_0001.csv
       ├─ Thread 1: generate N users → write users_0002.csv
       └─ Thread N: ...
            │
            ├─ SmallRng::from_entropy()（スレッドローカル乱数器）
            ├─ username 重複解決のみ Mutex<HashMap> で直列化
            └─ 電話番号は Arc<Vec<u32>> のスライスを参照（Mutex 不要）
```

### 設計のポイント

| 項目              | 採用技術                                        | 理由                          |
|-----------------|---------------------------------------------|-----------------------------|
| 並列生成            | `rayon`                                     | ゼロコスト並列イテレーター               |
| 乱数              | `SmallRng`（スレッドローカル）                        | Mutex 不要で高速                 |
| username 重複管理   | `Mutex<HashMap<String, u32>>`               | 全体で一意性保証                    |
| 重み付き選択          | `WeightedTable<T>`（累積和 + 二分探索）              | ロード時 O(n)・選択時 O(log n) で高速  |
| 電話番号の一意性保証      | Fisher-Yates partial shuffle → `Arc<Vec<u32>>` | Mutex・再試行ゼロ、O(1) アクセス        |
| CSV             | `csv` クレート                                  | 高速・RFC 4180 準拠              |
| 郵便番号            | `zip` クレート                                  | ZIP 直接展開・UTF-8 として読み込み      |
| 日付              | `chrono`                                    | 閏年・年齢計算が正確                  |


パフォーマンス目安
--------------------------------------------------------------------------------

| 件数    | コア数 | 目安時間  |
|-------|-----|-------|
| 100万  | 4   | ~3秒   |
| 500万  | 8   | ~8秒   |
| 1000万 | 16  | ~15秒  |

※ SSD 環境・郵便番号 CSV が 12 万件の場合の目安値
※ 起動時の電話番号インデックス生成（~1秒・メモリ使用量 ~40MB）を含む


注意事項
--------------------------------------------------------------------------------

- username の重複カウンタは `Mutex` で保護しているため、超大量生成（1000万件）でも正確な連番が付与されます。
- 生年月日の範囲：実行日から 12 歳以上 105 歳以下（誕生日当日を含む）。
- 電話番号は `050` / `070` / `080` / `090` で始まり、4桁目が `0` 固定の 11 桁形式です。生成可能な総パターン数は 4,000 万通りで、最大生成件数（1,000 万件）に対して十分な空間を確保しています。


townAreaName の加工ルール
--------------------------------------------------------------------------------

郵便番号 CSV の町域名は読み込み時・生成時に以下の順で加工されます。

### 読み込み時（`load_addresses`）

- 全角括弧 `（）` を除去し、括弧内の文字列は保持します。
- 波線文字 `～`（U+FF5E 全角チルダ）を `〜`（U+301C 波ダッシュ）に正規化します。  
  郵便番号 CSV は Shift_JIS 由来のため、変換方式によって文字コードが異なる場合があります。
- 「以下に掲載がない場合」を含む行は除外します。

### 生成時（`resolve_town_area`）

以下の優先順位で処理します。

1. **除去キーワード**
    - `地割`（`第ｎ地割` を含む）が含まれる場合、`地割` キーワード以降を切り捨てます。
    - `地階` が含まれる場合、`地階` 以降を切り捨てます。
2. **読点 `、`** が含まれる場合、候補に分割してランダムに 1 つ選択します（再帰）。
3. **中黒 `・`** の前後がいずれも `数字+番` / `数字+番地` の形式の場合のみ分割してランダムに 1 つ選択します（再帰）。建物名など番号以外の中黒はそのまま保持します。
4. **波線 `〜`** が含まれる場合、前後の数値を範囲として解釈しランダムに 1 つ選択します。複数の波線が含まれる場合は再帰的に処理します。数値が抽出できない場合は波線より左の文字列を採用して再帰します。
5. **番地情報がない**場合（数字・全角数字・「丁」を含まない）、全角数字・全角ハイフンで丁目・番地・号（各 1〜20）をランダムに付与します。
