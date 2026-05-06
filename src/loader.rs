use std::{collections::HashMap, fs::File, io::BufReader, path::PathBuf};

use anyhow::{Context, Result};
use zip::ZipArchive;

use crate::models::{Address, FamilyName, GivenName, WeightedTable};

// ============================================================
//  出現頻度 → 重み変換
// ============================================================

/// frequency 値（1〜5）を重みに変換する。
/// 重み = 1 / 8^(frequency - 1)
///   frequency=1 → 1.0
///   frequency=2 → 0.125
///   frequency=3 → 0.015625
///   frequency=4 → 0.001953125
///   frequency=5 → 0.000244140625
fn frequency_to_weight(freq: u8) -> f64 {
    1.0 / 8f64.powi((freq.saturating_sub(1)) as i32)
}

/// 波線の正規化: U+FF5E（～ 全角チルダ）を U+301C（〜 波ダッシュ）に統一する。
/// 郵便番号 CSV は Shift_JIS 由来のため ～(U+FF5E) が混在することがある。
pub fn normalize_tilde(s: &str) -> std::borrow::Cow<'_, str> {
    if s.contains('\u{FF5E}') {
        std::borrow::Cow::Owned(s.replace('\u{FF5E}', "\u{301C}"))
    } else {
        std::borrow::Cow::Borrowed(s)
    }
}

// ============================================================
//  名前 CSV 読み込み
// ============================================================

/// 姓 CSV を読み込む。
/// 期待するフォーマット: `kanji,hiragana,romaji,frequency`（ヘッダー行あり）
pub fn load_family_names(path: &PathBuf) -> Result<WeightedTable<FamilyName>> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_path(path)
        .with_context(|| format!("family_name CSV を開けません: {}", path.display()))?;

    let mut names = Vec::new();
    let mut weights = Vec::new();
    for result in rdr.records() {
        let rec = result?;
        let freq: u8 = rec.get(3).unwrap_or("1").trim().parse().unwrap_or(1).clamp(1, 3);
        names.push(FamilyName {
            kanji:    rec.get(0).unwrap_or("").trim().to_string(),
            hiragana: rec.get(1).unwrap_or("").trim().to_string(),
            romaji:   rec.get(2).unwrap_or("").trim().to_string(),
        });
        weights.push(frequency_to_weight(freq));
    }
    anyhow::ensure!(!names.is_empty(), "family_name CSV にレコードがありません");
    Ok(WeightedTable::build(names, weights))
}

/// 名 CSV を読み込む（男女共通ロジック）。
/// 期待するフォーマット: `kanji,hiragana,romaji,frequency`（ヘッダー行あり）
pub fn load_given_names(path: &PathBuf) -> Result<WeightedTable<GivenName>> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_path(path)
        .with_context(|| format!("given_name CSV を開けません: {}", path.display()))?;

    let mut names = Vec::new();
    let mut weights = Vec::new();
    for result in rdr.records() {
        let rec = result?;
        let freq: u8 = rec.get(3).unwrap_or("1").trim().parse().unwrap_or(1).clamp(1, 3);
        names.push(GivenName {
            kanji:    rec.get(0).unwrap_or("").trim().to_string(),
            hiragana: rec.get(1).unwrap_or("").trim().to_string(),
            romaji:   rec.get(2).unwrap_or("").trim().to_string(),
        });
        weights.push(frequency_to_weight(freq));
    }
    anyhow::ensure!(!names.is_empty(), "given_name CSV にレコードがありません");
    Ok(WeightedTable::build(names, weights))
}

// ============================================================
//  都道府県頻度 CSV 読み込み
// ============================================================

/// 都道府県別出現頻度 CSV を読み込み、都道府県名 → 重み のマップを返す。
/// 期待フォーマット: `prefecture,hiragana,romaji,population,frequency`（ヘッダー行あり）
/// 重み = 1 / 2^(frequency - 1)
pub fn load_ken_frequency(path: &PathBuf) -> Result<HashMap<String, f64>> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_path(path)
        .with_context(|| format!("ken_frequency CSV を開けません: {}", path.display()))?;

    let mut map = HashMap::new();
    for result in rdr.records() {
        let rec = result?;
        let prefecture = rec.get(0).unwrap_or("").trim().to_string();
        let freq: u8 = rec.get(4).unwrap_or("1").trim().parse().unwrap_or(1).clamp(1, 10);
        if !prefecture.is_empty() {
            let weight = 1.0f64 / 2f64.powi((freq.saturating_sub(1)) as i32);
            map.insert(prefecture, weight);
        }
    }
    anyhow::ensure!(!map.is_empty(), "ken_frequency CSV にレコードがありません");
    Ok(map)
}

// ============================================================
//  郵便番号 ZIP 読み込み
// ============================================================

/// utf_ken_all.zip を展開し、都道府県別出現頻度を反映した WeightedTable を返す。
/// 内部は WeightedTable<Vec<Address>>:
///   - 第1段階: 都道府県を重み付き選択
///   - 第2段階: 選ばれた都道府県内の住所を均等選択
/// 期待フォーマット（郵便番号 CSV）:
///   col[2]  = 7桁郵便番号
///   col[6]  = 都道府県名（漢字）
///   col[7]  = 市区町村名（漢字）
///   col[8]  = 町域名（漢字）
pub fn load_addresses(
    zip_path: &PathBuf,
    ken_freq: &HashMap<String, f64>,
) -> Result<WeightedTable<Vec<Address>>> {
    let file = File::open(zip_path)
        .with_context(|| format!("ZIP を開けません: {}", zip_path.display()))?;
    let mut archive = ZipArchive::new(BufReader::new(file))?;

    // ZIP 内の最初の .csv / .CSV ファイルを使う
    let csv_index = (0..archive.len())
        .find(|&i| {
            let name = archive.by_index(i).map(|f| f.name().to_lowercase()).unwrap_or_default();
            name.ends_with(".csv")
        })
        .context("ZIP 内に CSV ファイルが見つかりません")?;

    let csv_file = archive.by_index(csv_index)?;

    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(csv_file);

    // 都道府県名ごとに住所をグループ化
    let mut grouped: HashMap<String, Vec<Address>> = HashMap::new();
    for result in rdr.records() {
        let rec = result?;
        let postcode    = rec.get(2).unwrap_or("").trim().to_string();
        let prefecture  = rec.get(6).unwrap_or("").trim().to_string();
        let municipality = rec.get(7).unwrap_or("").trim().to_string();
        let raw_town_area = rec.get(8).unwrap_or("").trim().to_string();

        // 空行・不正行を除外
        if postcode.len() != 7 || prefecture.is_empty() {
            continue;
        }

        // 「以下に掲載がない場合」は除外
        if raw_town_area.contains("以下に掲載がない場合") {
            continue;
        }

        // 全角括弧「（」「）」のみ除去し、括弧内の文字列は残す。
        // 波線文字を U+301C に正規化（Shift_JIS 由来の U+FF5E 対策）。
        let town_area = normalize_tilde(
            &raw_town_area.replace('（', "").replace('）', "")
        ).into_owned();

        grouped.entry(prefecture.clone()).or_default().push(Address {
            postcode: format!("{}-{}", &postcode[..3], &postcode[3..]),
            prefecture,
            municipality,
            town_area,
        });
    }
    anyhow::ensure!(!grouped.is_empty(), "住所データが 0 件です");

    // 都道府県順に WeightedTable を構築
    // ken_frequency に載っていない都道府県はデフォルト重み 1.0 で扱う
    let mut items: Vec<Vec<Address>> = Vec::new();
    let mut weights: Vec<f64> = Vec::new();
    for (pref, addrs) in grouped {
        let w = *ken_freq.get(&pref).unwrap_or(&1.0);
        items.push(addrs);
        weights.push(w);
    }

    Ok(WeightedTable::build(items, weights))
}
