user_generator
================================================================================

CLI tool for rapidly generating random lists of Japanese users.
Supports multi-threading parallel generation & parallel CSV writing using Rayon.

> [!NOTE]
> This software is based on AI-generated code with additional implementation.


Build and Run
--------------------------------------------------------------------------------

```bash
# Build
cargo build --release

# Run
./target/release/user_generator --count 100000
```


Docker build and Run
--------------------------------------------------------------------------------

```bash
# Build
cargo build --release
docker build --tag user_generator \
--build-arg BIN=./target/release/user_generator \
--build-arg DATA_DIR=./data .

# Run
mkdir -p /tmp/out
docker run --rm \
-v /tmp/out:/var/output \
-e USERGEN__COUNT=10000 \
user_generator
```


--------------------------------------------------------------------------------


user_generator
================================================================================

ランダムな日本人ユーザーリストを高速生成する CLI ツールです。  
Rayon による **マルチスレッド並列生成 & 並列 CSV 書き込み** に対応しています。


ディレクトリ構成
--------------------------------------------------------------------------------

```
user_generator/
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

# バイナリは target/release/user_generator に生成される
```


使い方
--------------------------------------------------------------------------------

```bash
# 基本（デフォルト: 1000件、data/ ディレクトリのファイルを使用）
./target/release/user_generator

# 件数を指定
./target/release/user_generator --count 1000000

# ファイルパスをすべて明示指定
./target/release/user_generator \
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

| オプション                | 短縮   | デフォルト                        | 説明                   |
|----------------------|------|------------------------------|----------------------|
| `--count`            | `-c` | `1000`                       | 生成件数（最大 10,000,000）  |
| `--family-name`      |      | `data/family_name.csv`       | 姓 CSV パス             |
| `--given-name-male`  |      | `data/given_name_male.csv`   | 男性名 CSV パス           |
| `--given-name-female` |     | `data/given_name_female.csv` | 女性名 CSV パス           |
| `--ken-frequency`    |      | `data/ken_frequency.csv`     | 都道府県出現頻度 CSV パス      |
| `--ken-all`          |      | `data/utf_ken_all.zip`       | 郵便番号 ZIP パス          |
| `--output-dir`       | `-o` | `output`                     | 出力ディレクトリ             |
| `--chunk-size`       |      | `1,000,000`                  | ファイル分割単位             |
| `--threads`          |      | `0`（全コア）                     | 使用スレッド数              |


出力ファイル
--------------------------------------------------------------------------------

- **1 チャンクの場合** → `output/users.csv`
- **複数チャンクの場合** → `output/users_0001.csv`, `output/users_0002.csv`, ...

### CSV 列定義

| 列                    | 内容                                    |
|----------------------|---------------------------------------|
| `username`           | 姓ローマ字.名ローマ字（小文字）、重複時は連番付き            |
| `email`              | `username@example.com`                |
| `familyName`         | 姓（漢字）                                 |
| `familyNameHiragana` | 姓（ひらがな）                               |
| `familyNameRomaji`   | 姓（ローマ字）                               |
| `givenName`          | 名（漢字）                                 |
| `givenNameHiragana`  | 名（ひらがな）                               |
| `givenNameRomaji`    | 名（ローマ字）                               |
| `gender`             | 1=男性、2=女性                             |
| `birthDate`          | 生年月日（YYYY-MM-DD）、12〜105歳              |
| `postcode`           | 郵便番号（例: 100-0001）                     |
| `prefectureName`     | 都道府県名                                 |
| `municipalityName`   | 市区町村名                                 |
| `townAreaName`       | 町域名（番地なしの場合は全角で丁目・番地・号をランダム付与）        |


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
  └─ rayon::par_iter（チャンク単位で並列）
       ├─ Thread 0: generate N users → write users_0001.csv
       ├─ Thread 1: generate N users → write users_0002.csv
       └─ Thread N: ...
            │
            ├─ SmallRng::from_entropy()（スレッドローカル乱数器）
            └─ username 重複解決のみ Mutex<HashMap> で直列化
```

### 設計のポイント

| 項目            | 採用技術                          | 理由                          |
|---------------|-----------------------------------|-----------------------------|
| 並列生成          | `rayon`                           | ゼロコスト並列イテレーター               |
| 乱数            | `SmallRng`（スレッドローカル）             | Mutex 不要で高速                 |
| username 重複管理 | `Mutex<HashMap<String, u32>>`     | 全体で一意性保証                    |
| 重み付き選択        | `WeightedTable<T>`（累積和 + 二分探索）   | ロード時 O(n)・選択時 O(log n) で高速  |
| CSV           | `csv` クレート                       | 高速・RFC 4180 準拠              |
| 郵便番号          | `zip` クレート                       | ZIP 直接展開・UTF-8 として読み込み      |
| 日付            | `chrono`                          | 閏年・年齢計算が正確                  |


パフォーマンス目安
--------------------------------------------------------------------------------

| 件数    | コア数 | 目安時間 |
|-------|-----|------|
| 100万  | 4   | ~3秒  |
| 500万  | 8   | ~8秒  |
| 1000万 | 16  | ~12秒 |

※ SSD 環境・郵便番号 CSV が 12 万件の場合の目安値


注意事項
--------------------------------------------------------------------------------

- username の重複カウンタは `Mutex` で保護しているため、超大量生成（1000万件）でも正確な連番が付与されます。
- 生年月日の範囲：実行日から 12 歳以上 105 歳以下（誕生日当日を含む）。
- `townAreaName` が「以下に掲載がない場合」の行は読み込み時に除外されます。
- `townAreaName` の全角括弧 `（）` は除去されますが、括弧内の文字列は保持されます。
- `townAreaName` に番地情報がない場合、全角数字・全角ハイフンで丁目・番地・号（各 1〜20）をランダムに付与します。
