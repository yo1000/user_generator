use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use chrono::{Datelike, Duration, NaiveDate};
use rand::Rng;

use crate::models::{Address, FamilyName, GivenName, User, WeightedTable};

// ============================================================
//  username 重複管理
// ============================================================

/// username の重複カウンタ（スレッド間共有）
pub type UsernameCounter = Arc<Mutex<HashMap<String, u32>>>;

/// 重複を考慮した username を生成する
pub fn resolve_username(base: &str, counter: &UsernameCounter) -> String {
    let mut map = counter.lock().unwrap();
    let entry = map.entry(base.to_string()).or_insert(0);
    *entry += 1;
    if *entry == 1 {
        base.to_string()
    } else {
        format!("{}{}", base, entry)
    }
}

// ============================================================
//  電話番号
// ============================================================

/// 電話番号インデックス（0..PHONE_SPACE）を文字列に変換する。
/// 番号空間: {050|070|080|090} × 0{000..999} × {0000..9999} = 4,000万通り
///   index = prefix_idx * 10_000_000 + mid * 10_000 + tail
///   フォーマット例） 090-0123-4567
pub const PHONE_SPACE: u32 = 4 * 1_000 * 10_000; // 40_000_000
const PHONE_PREFIXES: [&str; 4] = ["050", "070", "080", "090"];

pub fn phone_index_to_string(idx: u32) -> String {
    let tail       = idx % 10_000;
    let mid        = (idx / 10_000) % 1_000;
    let prefix_idx = (idx / 10_000_000) as usize;
    format!("{}-0{:03}-{:04}", PHONE_PREFIXES[prefix_idx], mid, tail)
}

/// 必要件数分の非重複電話番号インデックスを Fisher-Yates partial shuffle で生成する。
/// Vec<u32> として返し、チャンクはスライスで受け取るため Mutex 不要。
pub fn generate_phone_indices(total: usize, rng: &mut impl Rng) -> Vec<u32> {
    assert!(total <= PHONE_SPACE as usize, "生成件数が電話番号空間を超えています");
    // 0..PHONE_SPACE の先頭 total 要素だけをシャッフルする部分的 Fisher-Yates
    let mut pool: Vec<u32> = (0..PHONE_SPACE).collect();
    for i in 0..total {
        let j = rng.gen_range(i..PHONE_SPACE as usize);
        pool.swap(i, j);
    }
    pool.truncate(total);
    pool
}

// ============================================================
//  住所文字列の解釈
// ============================================================

/// 半角数字を全角数字に変換する。
fn to_fullwidth(n: u8) -> String {
    n.to_string()
        .chars()
        .map(|c| char::from_u32('０' as u32 + (c as u32 - '0' as u32)).unwrap_or(c))
        .collect()
}

/// 全角数字を半角数字に変換する（数値パース用）。
fn from_fullwidth_digits(s: &str) -> String {
    s.chars()
        .map(|c| {
            if ('０'..='９').contains(&c) {
                (b'0' + (c as u32 - '０' as u32) as u8) as char
            } else {
                c
            }
        })
        .collect()
}

/// 全角・半角数字からなる文字列が「番」または「番地」で終わるか判定する。
/// 中黒「・」前後の分割が番号表現かどうかの判定に使用。
fn is_ban_token(s: &str) -> bool {
    let s = s.trim();
    let suffix = if s.ends_with("番地") {
        &s[..s.len() - "番地".len()]
    } else if s.ends_with('番') {
        &s[..s.len() - '番'.len_utf8()]
    } else {
        return false;
    };
    !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit() || ('０'..='９').contains(&c))
}

/// 波線「〜」を挟む左辺・右辺から数値範囲を抽出して、
/// [lo, hi] のランダムな値を全角数字で返す。
/// 失敗した場合は None。
fn pick_tilde_range(s: &str, rng: &mut impl Rng) -> Option<String> {
    let tilde_pos = s.find('〜')?;
    let left  = &s[..tilde_pos];
    let right = &s[tilde_pos + '〜'.len_utf8()..];

    let left_ascii  = from_fullwidth_digits(left);
    let right_ascii = from_fullwidth_digits(right);

    let left_num_str: String = left_ascii.chars().rev()
        .take_while(|c| c.is_ascii_digit()).collect::<String>()
        .chars().rev().collect();
    let right_num_str: String = right_ascii.chars()
        .take_while(|c| c.is_ascii_digit()).collect();

    let lo = left_num_str.parse::<u32>().ok()?;
    let hi = right_num_str.parse::<u32>().ok()?;
    if lo > hi { return None; }

    // prefix（left の末尾数字を除いた部分）
    let num_chars_left = left.chars().rev()
        .take_while(|c| ('０'..='９').contains(c) || c.is_ascii_digit())
        .count();
    let prefix_end = left.char_indices()
        .nth(left.chars().count() - num_chars_left)
        .map(|(i, _)| i)
        .unwrap_or(left.len());
    let prefix = &left[..prefix_end];

    // suffix（right の先頭数字を除いた部分）
    let num_chars_right = right.chars()
        .take_while(|c| ('０'..='９').contains(c) || c.is_ascii_digit())
        .count();
    let suffix_start = right.char_indices()
        .nth(num_chars_right)
        .map(|(i, _)| i)
        .unwrap_or(right.len());
    let suffix = &right[suffix_start..];

    let chosen = rng.gen_range(lo..=hi);
    let chosen_fw: String = chosen.to_string().chars()
        .map(|c| char::from_u32('０' as u32 + (c as u32 - '0' as u32)).unwrap_or(c))
        .collect();
    Some(format!("{}{}{}", prefix, chosen_fw, suffix))
}

/// 「第ｎ地割」または「地割」の開始バイト位置を返す。
fn find_chiwari_pos(s: &str) -> Option<usize> {
    let pos = s.find("地割")?;
    let before = &s[..pos];

    // before を文字のリストとして収集し、末尾から検索する
    let chars: Vec<char> = before.chars().collect();
    let total = chars.len();

    // 末尾から連続する全角・半角数字を数える
    let num_count = chars.iter().rev()
        .take_while(|&&c| ('０'..='９').contains(&c) || c.is_ascii_digit())
        .count();

    // 数字の直前が「第」かどうか確認
    let dai_idx = total.checked_sub(num_count + 1);
    let dai_present = dai_idx.map(|i| chars[i] == '第').unwrap_or(false);

    if num_count > 0 && dai_present {
        // 「第」のバイト位置を char_indices で取得
        let dai_char_idx = total - num_count - 1;
        let start = before.char_indices()
            .nth(dai_char_idx)
            .map(|(i, _)| i)
            .unwrap_or(pos);
        Some(start)
    } else {
        Some(pos)
    }
}

/// town_area 文字列を解釈してランダムに 1 つの値を返す。
///
/// 処理の優先順位:
///   0. 除去キーワード:
///      - 「地割」が含まれる → 「地割」の直前の文字（「第ｎ」等）ごと、
///        「地割」以降の文字列（「第ｎ地割…」全体）を除去
///      - 「地階」が含まれる → 「地階」以降を除去
///   1. 読点「、」が含まれる → 候補に分割してランダムに 1 つ選択（再帰）
///   2. 中黒「・」が含まれ、前後が「数字+番(地)」トークンの場合 →
///        各トークンに再帰適用した結果からランダムに 1 つ選択
///   3. 波線「〜」が含まれる → 数値範囲からランダムに選択。
///        結果に波線が残る場合は再帰。失敗した場合は左辺を採用して再帰。
///   4. どれも該当しない → そのまま返す
pub fn resolve_town_area(s: &str, rng: &mut impl Rng) -> String {
    // ── 0a. 「地割」除去 ─────────────────────────────────────
    let s = if let Some(pos) = find_chiwari_pos(s) { &s[..pos] } else { s };

    // ── 0b. 「地階」除去 ─────────────────────────────────────
    let s = if let Some(pos) = s.find("地階") { &s[..pos] } else { s };

    let s = s.trim();
    if s.is_empty() {
        return s.to_string();
    }

    // ── 1. 読点分割（再帰） ──────────────────────────────────
    if s.contains('、') {
        let parts: Vec<&str> = s.split('、').collect();
        let chosen = parts[rng.gen_range(0..parts.len())];
        return resolve_town_area(chosen, rng);
    }

    // ── 2. 中黒「・」の前後が「数字+番(地)」の場合のみ分割 ──
    if s.contains('・') {
        let parts: Vec<&str> = s.split('・').collect();
        let all_ban = parts.iter().all(|p| {
            is_ban_token(p) || (p.contains('〜') && {
                let ti = p.find('〜').unwrap();
                let l = &p[..ti];
                let r = &p[ti + '〜'.len_utf8()..];
                from_fullwidth_digits(l).chars().rev().next()
                    .map(|c| c.is_ascii_digit()).unwrap_or(false)
                    && from_fullwidth_digits(r).chars().next()
                    .map(|c| c.is_ascii_digit()).unwrap_or(false)
            })
        });
        if all_ban {
            let chosen = parts[rng.gen_range(0..parts.len())];
            return resolve_town_area(chosen, rng);
        }
    }

    // ── 3. 波線範囲選択 ──────────────────────────────────────
    if s.contains('〜') {
        if let Some(result) = pick_tilde_range(s, rng) {
            return if result.contains('〜') {
                resolve_town_area(&result, rng)
            } else {
                result
            };
        }
        let left = &s[..s.find('〜').unwrap()];
        return resolve_town_area(left, rng);
    }

    // ── 4. そのまま返す ─────────────────────────────────────
    s.to_string()
}

// ============================================================
//  丁目・番地・号の付与
// ============================================================

/// 丁目・番地・号をランダム生成する（数字・ハイフンはすべて全角）。
/// - depth 0: 丁目のみ          例）「３」
/// - depth 1: 丁目－番地        例）「３－７」
/// - depth 2: 丁目－番地－号    例）「３－７－１２」
pub fn random_street_number(rng: &mut impl Rng) -> String {
    let depth  = rng.gen_range(0..=2usize);
    let chome  = to_fullwidth(rng.gen_range(1u8..=20));
    let banchi = to_fullwidth(rng.gen_range(1u8..=20));
    let go     = to_fullwidth(rng.gen_range(1u8..=20));
    match depth {
        0 => chome,
        1 => format!("{}－{}", chome, banchi),
        _ => format!("{}－{}－{}", chome, banchi, go),
    }
}

// ============================================================
//  生年月日
// ============================================================

/// 生年月日をランダム生成（12歳以上 105歳以下）
fn random_birth_date(rng: &mut impl Rng, today: NaiveDate) -> String {
    let min_date = today - Duration::days(105 * 365 + 26); // 最年長
    let max_date = today - Duration::days(12 * 365 + 3);   // 最年少

    let min_ord = min_date.num_days_from_ce();
    let max_ord = max_date.num_days_from_ce();
    let rand_ord = rng.gen_range(min_ord..=max_ord);
    NaiveDate::from_num_days_from_ce_opt(rand_ord)
        .map(|d| format!("{}-{:02}-{:02}", d.year(), d.month(), d.day()))
        .unwrap_or_else(|| "1990-01-01".to_string())
}

// ============================================================
//  ユーザー生成
// ============================================================

/// ユーザー 1 件を生成する
pub fn generate_user(
    rng: &mut impl Rng,
    family_names: &WeightedTable<FamilyName>,
    male_names: &WeightedTable<GivenName>,
    female_names: &WeightedTable<GivenName>,
    addresses: &WeightedTable<Vec<Address>>,
    today: NaiveDate,
    username_counter: &UsernameCounter,
    phone_number_idx: u32,
) -> User {
    // 性別
    let gender: u8 = if rng.gen_bool(0.5) { 1 } else { 2 };

    // 重み付きランダム選択
    let fam = family_names.sample(rng);
    let giv = if gender == 1 { male_names.sample(rng) } else { female_names.sample(rng) };

    // username ベース: 姓ローマ字.名ローマ字（小文字）
    let base_username = format!(
        "{}.{}",
        fam.romaji.to_lowercase(),
        giv.romaji.to_lowercase()
    );
    let username = resolve_username(&base_username, username_counter);
    let email = format!("{}@example.com", username);

    // 住所: 第1段階=都道府県を重み付き選択、第2段階=その都道府県内を均等選択
    let pref_addrs = addresses.sample(rng);
    let addr = &pref_addrs[rng.gen_range(0..pref_addrs.len())];

    // 生年月日
    let birth_date = random_birth_date(rng, today);

    // 電話番号（事前生成済みインデックスから変換）
    let phone_number = phone_index_to_string(phone_number_idx);

    // town_area の解釈と番地付与
    let town_area_name = {
        let ta = resolve_town_area(&addr.town_area, rng);
        let has_number = ta.chars().any(|c| c.is_ascii_digit())
            || ta.chars().any(|c| ('０'..='９').contains(&c))
            || ta.contains('丁');
        if has_number { ta } else { format!("{}{}", ta, random_street_number(rng)) }
    };

    User {
        username,
        email,
        family_name: fam.kanji.clone(),
        family_name_hiragana: fam.hiragana.clone(),
        family_name_romaji: fam.romaji.clone(),
        given_name: giv.kanji.clone(),
        given_name_hiragana: giv.hiragana.clone(),
        given_name_romaji: giv.romaji.clone(),
        gender,
        birth_date,
        phone_number,
        postcode: addr.postcode.clone(),
        prefecture_name: addr.prefecture.clone(),
        municipality_name: addr.municipality.clone(),
        town_area_name,
    }
}

// ============================================================
//  テスト
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    /// 固定シードの RNG を返す（テストの再現性を保証）
    fn rng() -> SmallRng { SmallRng::seed_from_u64(42) }

    // ----------------------------------------------------------
    //  from_fullwidth_digits
    // ----------------------------------------------------------

    #[test]
    fn test_from_fullwidth_digits_all_fullwidth() {
        assert_eq!(from_fullwidth_digits("１２３"), "123");
    }

    #[test]
    fn test_from_fullwidth_digits_mixed() {
        // 全角・半角・非数字が混在しても非数字はそのまま
        assert_eq!(from_fullwidth_digits("西４線49"), "西4線49");
    }

    #[test]
    fn test_from_fullwidth_digits_no_digits() {
        assert_eq!(from_fullwidth_digits("番地"), "番地");
    }

    // ----------------------------------------------------------
    //  is_ban_token
    // ----------------------------------------------------------

    #[test]
    fn test_is_ban_token_fullwidth_ban() {
        assert!(is_ban_token("１番"));
        assert!(is_ban_token("１０番"));
        assert!(is_ban_token("２７番"));
    }

    #[test]
    fn test_is_ban_token_banti() {
        assert!(is_ban_token("４９番地"));
        assert!(is_ban_token("７８番地"));
    }

    #[test]
    fn test_is_ban_token_not_number() {
        // 数字のない「番」は番号表現ではない
        assert!(!is_ban_token("番"));
        assert!(!is_ban_token("番地"));
    }

    #[test]
    fn test_is_ban_token_building_name() {
        // 建物名など数字なしは false
        assert!(!is_ban_token("南館"));
        assert!(!is_ban_token("花川町"));
    }

    // ----------------------------------------------------------
    //  find_chiwari_pos
    // ----------------------------------------------------------

    #[test]
    fn test_find_chiwari_pos_dai_n_with_prefix() {
        // 「前住所＋第ｎ地割」→ 「第」より前の住所部分のバイト位置を返す
        let s = "上野第２地割「９２」";
        let pos = find_chiwari_pos(s).unwrap();
        assert_eq!(&s[..pos], "上野");
    }

    #[test]
    fn test_find_chiwari_pos_dai_n_only() {
        // 「第ｎ地割」のみ（先頭から地割）→ 先頭バイト位置 0 を返す
        let s = "第２地割「９２」";
        let pos = find_chiwari_pos(s).unwrap();
        assert_eq!(&s[..pos], "");
    }

    #[test]
    fn test_find_chiwari_pos_standalone() {
        // 単独「地割」
        let s = "上野地割";
        let pos = find_chiwari_pos(s).unwrap();
        assert_eq!(&s[..pos], "上野");
    }

    #[test]
    fn test_find_chiwari_pos_none() {
        assert!(find_chiwari_pos("六本木６丁目").is_none());
    }

    // ----------------------------------------------------------
    //  pick_tilde_range
    // ----------------------------------------------------------

    #[test]
    fn test_pick_tilde_range_basic() {
        let mut rng = rng();
        for _ in 0..20 {
            let result = pick_tilde_range("４９〜７８番地", &mut rng).unwrap();
            // 結果は「XX番地」形式で波線を含まない
            assert!(!result.contains('〜'), "波線が残っている: {result}");
            assert!(result.ends_with("番地"), "suffix が欠落: {result}");
        }
    }

    #[test]
    fn test_pick_tilde_range_range_bounds() {
        let mut rng = rng();
        for _ in 0..50 {
            let result = pick_tilde_range("４９〜７８番地", &mut rng).unwrap();
            // 数値部分を抽出して範囲内か確認
            let num: u32 = from_fullwidth_digits(result.trim_end_matches("番地"))
                .parse()
                .unwrap();
            assert!((49..=78).contains(&num), "範囲外: {num}");
        }
    }

    #[test]
    fn test_pick_tilde_range_with_line_prefix() {
        // 「西５線」のようなプレフィックスが残るケース
        let mut rng = rng();
        let result = pick_tilde_range("美栄町西５線７９〜１１０番地", &mut rng).unwrap();
        assert!(result.starts_with("美栄町西５線"), "prefix が欠落: {result}");
        assert!(result.ends_with("番地"), "suffix が欠落: {result}");
    }

    #[test]
    fn test_pick_tilde_range_lo_greater_than_hi_returns_none() {
        let mut rng = rng();
        // lo > hi は None
        assert!(pick_tilde_range("７８〜４９番地", &mut rng).is_none());
    }

    #[test]
    fn test_pick_tilde_range_no_number_returns_none() {
        let mut rng = rng();
        // 数値が抽出できない場合は None
        assert!(pick_tilde_range("花川町〜花川南", &mut rng).is_none());
    }

    // ----------------------------------------------------------
    //  resolve_town_area — 基本
    // ----------------------------------------------------------

    #[test]
    fn test_resolve_town_area_plain() {
        let mut rng = rng();
        let result = resolve_town_area("六本木６丁目", &mut rng);
        assert_eq!(result, "六本木６丁目");
    }

    #[test]
    fn test_resolve_town_area_no_tilde_remains() {
        // どのケースも波線が結果に残ってはいけない
        let cases = [
            "美栄町西５線７９〜１１０番地",
            "美栄町西６線７９〜１１０番地",
            "富士町西４線４９〜７８番地",
            "富士町西７線４９〜７８番地",
            "富士町西８線４９〜７８番地",
        ];
        for s in cases {
            let mut rng = rng();
            let result = resolve_town_area(s, &mut rng);
            assert!(!result.contains('〜'), "波線残存 [{s}] → [{result}]");
        }
    }

    // ----------------------------------------------------------
    //  resolve_town_area — 元データ由来の問題ケース
    //  （以前バグを起こした実データを必ず含む）
    // ----------------------------------------------------------

    /// 括弧除去・normalize_tilde 後の実データ相当
    /// 元データ: 富士町（西４～８線４９～７８番地）
    /// 処理後  : 富士町西４〜８線４９〜７８番地
    #[test]
    fn test_resolve_town_area_double_tilde_recursive() {
        let input = "富士町西４〜８線４９〜７８番地";
        for seed in 0..30 {
            let mut rng = SmallRng::seed_from_u64(seed);
            let result = resolve_town_area(input, &mut rng);
            assert!(!result.contains('〜'),
                    "波線残存 seed={seed} [{input}] → [{result}]");
            assert!(result.starts_with("富士町西"),
                    "prefix 欠落 seed={seed} [{result}]");
            assert!(result.ends_with("番地"),
                    "suffix 欠落 seed={seed} [{result}]");
        }
    }

    /// U+FF5E（全角チルダ）が normalize_tilde を経由せず残った場合の保険
    /// load_addresses で正規化済みのはずだが、回帰テストとして保持
    #[test]
    fn test_resolve_town_area_ff5e_tilde_via_loader_normalization() {
        // loader::normalize_tilde で U+301C に変換されていることを前提とする
        // ここでは変換済み文字列で resolve が正しく動くことを確認
        let normalized = "富士町西４\u{301C}８線４９\u{301C}７８番地";
        let mut rng = rng();
        let result = resolve_town_area(normalized, &mut rng);
        assert!(!result.contains('\u{301C}'), "波線残存: {result}");
        assert!(!result.contains('\u{FF5E}'), "FF5E 残存: {result}");
    }

    // ----------------------------------------------------------
    //  resolve_town_area — 読点
    // ----------------------------------------------------------

    #[test]
    fn test_resolve_town_area_ten_ten() {
        // 読点分割後の候補のいずれかが返る
        let candidates = ["泉が丘", "泉北高速鉄道以東"];
        let input = "泉が丘、泉北高速鉄道以東";
        for _ in 0..30 {
            let mut rng = rng();
            let result = resolve_town_area(input, &mut rng);
            assert!(candidates.contains(&result.as_str()),
                    "候補外の値: {result}");
        }
    }

    // ----------------------------------------------------------
    //  resolve_town_area — 中黒
    // ----------------------------------------------------------

    #[test]
    fn test_resolve_town_area_nakaguro_ban() {
        // 「数字+番」形式のみ中黒で分割される
        let input = "１番・１０〜２７番";
        for _ in 0..30 {
            let mut rng = rng();
            let result = resolve_town_area(input, &mut rng);
            assert!(!result.contains('〜'), "波線残存: {result}");
            assert!(!result.contains('・'), "中黒残存: {result}");
        }
    }

    #[test]
    fn test_resolve_town_area_nakaguro_building_name_kept() {
        // 建物名の中黒は分割されない
        let input = "東京ミッドタウン・タワー";
        let mut rng = rng();
        let result = resolve_town_area(input, &mut rng);
        assert_eq!(result, "東京ミッドタウン・タワー");
    }

    // ----------------------------------------------------------
    //  resolve_town_area — 地割・地階の除去
    // ----------------------------------------------------------

    #[test]
    fn test_resolve_town_area_chiwari_removed() {
        let cases = [
            "第２地割「９２」〜第４地割「３〜１１」",
            "第２地割「９６」〜第４地割「３〜１１」",
            "第２地割「９８」〜第４地割「３〜１１」",
            "第２地割「１０４」〜第４地割「３〜１１」",
        ];
        for s in cases {
            let mut rng = rng();
            let result = resolve_town_area(s, &mut rng);
            assert!(!result.contains("地割"), "地割残存 [{s}] → [{result}]");
            assert!(!result.contains('〜'), "波線残存 [{s}] → [{result}]");
        }
    }

    #[test]
    fn test_resolve_town_area_chikai_removed() {
        let input = "地階・階層不明";
        let mut rng = rng();
        let result = resolve_town_area(input, &mut rng);
        assert!(!result.contains("地階"), "地階残存: {result}");
    }
}
