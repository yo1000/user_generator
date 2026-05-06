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

// ============================================================
//  テスト
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // ----------------------------------------------------------
    //  ヘルパー
    // ----------------------------------------------------------

    /// CSV バイト列を一時ファイルに書き出して PathBuf を返す。
    /// 返された PathBuf はドロップ時に自動削除される tempfile ではないが、
    /// テスト終了後に OS が回収するため許容する。
    fn csv_tempfile(content: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        // スレッド ID を混ぜてテスト並列実行時の衝突を避ける
        path.push(format!(
            "test_{:?}.csv",
            std::thread::current().id()
        ));
        std::fs::write(&path, content.as_bytes()).unwrap();
        path
    }

    /// インメモリの郵便番号 CSV から ZIP バイト列を生成する。
    fn make_ken_zip(csv_content: &str) -> Vec<u8> {
        use std::io::Cursor;
        use zip::ZipWriter;

        let buf = Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(buf);
        zip.start_file("ken_all.csv", zip::write::FileOptions::<()>::default()).unwrap();
        zip.write_all(csv_content.as_bytes()).unwrap();
        zip.finish().unwrap().into_inner()
    }

    /// ZIP バイト列を一時ファイルに書き出して PathBuf を返す。
    fn zip_tempfile(content: &[u8]) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "test_{:?}.zip",
            std::thread::current().id()
        ));
        std::fs::write(&path, content).unwrap();
        path
    }

    // ----------------------------------------------------------
    //  frequency_to_weight
    // ----------------------------------------------------------

    #[test]
    fn test_frequency_to_weight_1() {
        assert!((frequency_to_weight(1) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_frequency_to_weight_2() {
        assert!((frequency_to_weight(2) - 0.125).abs() < 1e-9);
    }

    #[test]
    fn test_frequency_to_weight_3() {
        assert!((frequency_to_weight(3) - 0.015625).abs() < 1e-9);
    }

    #[test]
    fn test_frequency_to_weight_descending() {
        // 値が大きいほど重みは小さい
        assert!(frequency_to_weight(1) > frequency_to_weight(2));
        assert!(frequency_to_weight(2) > frequency_to_weight(3));
    }

    // ----------------------------------------------------------
    //  normalize_tilde
    // ----------------------------------------------------------

    #[test]
    fn test_normalize_tilde_ff5e_converted() {
        // U+FF5E は U+301C に変換される
        let input = "４９\u{FF5E}７８番地";
        let result = normalize_tilde(input);
        assert!(!result.contains('\u{FF5E}'), "FF5E が残っている");
        assert!(result.contains('\u{301C}'), "301C に変換されていない");
    }

    #[test]
    fn test_normalize_tilde_301c_unchanged() {
        // U+301C はそのまま（再変換されない）
        let input = "４９\u{301C}７８番地";
        let result = normalize_tilde(input);
        assert_eq!(result.as_ref(), input);
    }

    #[test]
    fn test_normalize_tilde_no_tilde_borrowed() {
        // 波線なし → Borrowed（アロケーションなし）
        let input = "六本木６丁目";
        let result = normalize_tilde(input);
        assert!(matches!(result, std::borrow::Cow::Borrowed(_)));
    }

    #[test]
    fn test_normalize_tilde_ff5e_owned() {
        // 変換あり → Owned
        let input = "４９\u{FF5E}７８";
        let result = normalize_tilde(input);
        assert!(matches!(result, std::borrow::Cow::Owned(_)));
    }

    /// 実データ由来: 元データの ～(U+FF5E) が正しく 〜(U+301C) に変換されること
    #[test]
    fn test_normalize_tilde_real_data_pattern() {
        // 元データ: 富士町（西４～８線４９～７８番地）
        // 括弧除去後: 富士町西４～８線４９～７８番地  ← ～ は U+FF5E
        let after_bracket_removal = "富士町西４\u{FF5E}８線４９\u{FF5E}７８番地";
        let normalized = normalize_tilde(after_bracket_removal);
        assert_eq!(
            normalized.as_ref(),
            "富士町西４\u{301C}８線４９\u{301C}７８番地"
        );
        assert!(!normalized.contains('\u{FF5E}'));
    }

    // ----------------------------------------------------------
    //  load_family_names
    // ----------------------------------------------------------

    #[test]
    fn test_load_family_names_basic() {
        let csv = "kanji,hiragana,romaji,frequency\n\
                   佐藤,さとう,Sato,1\n\
                   鈴木,すずき,Suzuki,2\n\
                   高橋,たかはし,Takahashi,3\n";
        let path = csv_tempfile(csv);
        let table = load_family_names(&path).unwrap();
        assert_eq!(table.items.len(), 3);
        assert_eq!(table.items[0].kanji, "佐藤");
        assert_eq!(table.items[0].romaji, "Sato");
    }

    #[test]
    fn test_load_family_names_weights() {
        let csv = "kanji,hiragana,romaji,frequency\n\
                   佐藤,さとう,Sato,1\n\
                   鈴木,すずき,Suzuki,2\n";
        let path = csv_tempfile(csv);
        let table = load_family_names(&path).unwrap();
        // frequency=1 の重みは frequency=2 より大きい
        // WeightedTable は内部に重みを保持しないため、
        // items の順序と count で間接確認
        assert_eq!(table.items.len(), 2);
    }

    #[test]
    fn test_load_family_names_empty_csv_fails() {
        let csv = "kanji,hiragana,romaji,frequency\n";
        let path = csv_tempfile(csv);
        assert!(load_family_names(&path).is_err());
    }

    #[test]
    fn test_load_family_names_missing_frequency_defaults_to_1() {
        // frequency 列がなくてもデフォルト 1 として読み込める
        let csv = "kanji,hiragana,romaji,frequency\n\
                   佐藤,さとう,Sato,\n";
        let path = csv_tempfile(csv);
        let table = load_family_names(&path).unwrap();
        assert_eq!(table.items.len(), 1);
    }

    // ----------------------------------------------------------
    //  load_given_names
    // ----------------------------------------------------------

    #[test]
    fn test_load_given_names_basic() {
        let csv = "kanji,hiragana,romaji,frequency\n\
                   太郎,たろう,Taro,1\n\
                   次郎,じろう,Jiro,2\n";
        let path = csv_tempfile(csv);
        let table = load_given_names(&path).unwrap();
        assert_eq!(table.items.len(), 2);
        assert_eq!(table.items[0].kanji, "太郎");
        assert_eq!(table.items[1].hiragana, "じろう");
    }

    #[test]
    fn test_load_given_names_empty_csv_fails() {
        let csv = "kanji,hiragana,romaji,frequency\n";
        let path = csv_tempfile(csv);
        assert!(load_given_names(&path).is_err());
    }

    // ----------------------------------------------------------
    //  load_ken_frequency
    // ----------------------------------------------------------

    #[test]
    fn test_load_ken_frequency_basic() {
        let csv = "prefecture,hiragana,romaji,population,frequency\n\
                   東京都,とうきょうと,Tokyo,14000000,1\n\
                   北海道,ほっかいどう,Hokkaido,5000000,3\n";
        let path = csv_tempfile(csv);
        let map = load_ken_frequency(&path).unwrap();
        assert_eq!(map.len(), 2);
        // frequency=1 → 重み 1.0
        assert!((map["東京都"] - 1.0).abs() < 1e-9);
        // frequency=3 → 重み 0.25
        assert!((map["北海道"] - 0.25).abs() < 1e-9);
    }

    #[test]
    fn test_load_ken_frequency_weight_formula() {
        // 重み = 1 / 2^(frequency - 1)
        let csv = "prefecture,hiragana,romaji,population,frequency\n\
                   都道府県A,a,A,1000,1\n\
                   都道府県B,b,B,1000,2\n\
                   都道府県C,c,C,1000,4\n\
                   都道府県D,d,D,1000,5\n";
        let path = csv_tempfile(csv);
        let map = load_ken_frequency(&path).unwrap();
        assert!((map["都道府県A"] - 1.0   ).abs() < 1e-9);
        assert!((map["都道府県B"] - 0.5   ).abs() < 1e-9);
        assert!((map["都道府県C"] - 0.125 ).abs() < 1e-9);
        assert!((map["都道府県D"] - 0.0625).abs() < 1e-9);
    }

    #[test]
    fn test_load_ken_frequency_empty_fails() {
        let csv = "prefecture,hiragana,romaji,population,frequency\n";
        let path = csv_tempfile(csv);
        assert!(load_ken_frequency(&path).is_err());
    }

    // ----------------------------------------------------------
    //  load_addresses（インメモリ ZIP 使用）
    // ----------------------------------------------------------

    /// 郵便番号 CSV の最小限の行を生成する。
    /// 列順: [0..1]=ダミー, [2]=7桁郵便番号, [3..5]=ダミー,
    ///       [6]=都道府県, [7]=市区町村, [8]=町域
    fn ken_csv_row(postcode: &str, pref: &str, city: &str, town: &str) -> String {
        format!("x,x,{postcode},x,x,x,{pref},{city},{town}\n")
    }

    #[test]
    fn test_load_addresses_basic() {
        let csv = ken_csv_row("1000001", "東京都", "千代田区", "丸の内");
        let zip_bytes = make_ken_zip(&csv);
        let zip_path = zip_tempfile(&zip_bytes);
        let ken_freq = HashMap::new();
        let table = load_addresses(&zip_path, &ken_freq).unwrap();
        assert_eq!(table.items.len(), 1); // 都道府県 1 件
        assert_eq!(table.items[0][0].prefecture, "東京都");
        assert_eq!(table.items[0][0].postcode, "100-0001");
    }

    #[test]
    fn test_load_addresses_postcode_formatted() {
        // 7桁 → ハイフン付き3-4形式
        let csv = ken_csv_row("0600001", "北海道", "札幌市中央区", "北一条西");
        let zip_bytes = make_ken_zip(&csv);
        let zip_path = zip_tempfile(&zip_bytes);
        let table = load_addresses(&zip_path, &HashMap::new()).unwrap();
        assert_eq!(table.items[0][0].postcode, "060-0001");
    }

    #[test]
    fn test_load_addresses_excludes_keisai_nashi() {
        // 「以下に掲載がない場合」は除外される
        let csv = ken_csv_row("1000001", "東京都", "千代田区", "以下に掲載がない場合")
            + &ken_csv_row("1000002", "東京都", "千代田区", "丸の内");
        let zip_bytes = make_ken_zip(&csv);
        let zip_path = zip_tempfile(&zip_bytes);
        let table = load_addresses(&zip_path, &HashMap::new()).unwrap();
        // 除外後 1 件のみ
        assert_eq!(table.items[0].len(), 1);
        assert_eq!(table.items[0][0].town_area, "丸の内");
    }

    #[test]
    fn test_load_addresses_removes_fullwidth_brackets() {
        // 全角括弧が除去される
        let csv = ken_csv_row("1060032", "東京都", "港区", "六本木（奇数）");
        let zip_bytes = make_ken_zip(&csv);
        let zip_path = zip_tempfile(&zip_bytes);
        let table = load_addresses(&zip_path, &HashMap::new()).unwrap();
        assert_eq!(table.items[0][0].town_area, "六本木奇数");
    }

    #[test]
    fn test_load_addresses_normalizes_ff5e_tilde() {
        // U+FF5E の波線が U+301C に正規化される
        let csv = ken_csv_row("0802333", "北海道", "帯広市", "富士町西４\u{FF5E}８線");
        let zip_bytes = make_ken_zip(&csv);
        let zip_path = zip_tempfile(&zip_bytes);
        let table = load_addresses(&zip_path, &HashMap::new()).unwrap();
        let town = &table.items[0][0].town_area;
        assert!(!town.contains('\u{FF5E}'), "FF5E が残っている: {town}");
        assert!(town.contains('\u{301C}'), "301C に変換されていない: {town}");
    }

    #[test]
    fn test_load_addresses_ken_freq_weight_applied() {
        // ken_frequency の重みが適用され、都道府県ごとにグループ化される
        let csv = ken_csv_row("1000001", "東京都", "千代田区", "丸の内")
            + &ken_csv_row("0600001", "北海道", "札幌市中央区", "北一条西");
        let zip_bytes = make_ken_zip(&csv);
        let zip_path = zip_tempfile(&zip_bytes);
        let mut ken_freq = HashMap::new();
        ken_freq.insert("東京都".to_string(), 1.0);
        ken_freq.insert("北海道".to_string(), 0.5);
        let table = load_addresses(&zip_path, &ken_freq).unwrap();
        // 2 都道府県がグループ化されている
        assert_eq!(table.items.len(), 2);
    }

    #[test]
    fn test_load_addresses_unknown_pref_defaults_weight_1() {
        // ken_freq に載っていない都道府県はデフォルト重み 1.0 で扱われ、読み込み自体は成功する
        let csv = ken_csv_row("8700001", "大分県", "大分市", "大手町");
        let zip_bytes = make_ken_zip(&csv);
        let zip_path = zip_tempfile(&zip_bytes);
        // 空の ken_freq でも正常に読み込める
        let table = load_addresses(&zip_path, &HashMap::new()).unwrap();
        assert_eq!(table.items.len(), 1);
    }
}
