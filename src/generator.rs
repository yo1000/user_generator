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
    if let Some(pos) = s.find("地割") {
        let before = &s[..pos];
        let mut chars_rev = before.chars().rev();
        let num_count = chars_rev
            .by_ref()
            .take_while(|c| ('０'..='９').contains(c) || c.is_ascii_digit())
            .count();
        let dai_present = chars_rev.next() == Some('第');
        let start = if num_count > 0 && dai_present {
            before.char_indices().rev()
                .nth(num_count)
                .map(|(i, _)| i)
                .unwrap_or(pos)
        } else {
            pos
        };
        return Some(start);
    }
    None
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
