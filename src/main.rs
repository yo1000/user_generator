use anyhow::{Context, Result};
use chrono::{Datelike, Duration, Local, NaiveDate};
use clap::Parser;
use csv::Writer;
use rand::prelude::*;
use rand::rngs::SmallRng;
use rayon::prelude::*;
use std::{
    collections::HashMap,
    fs::File,
    io::BufReader,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use zip::ZipArchive;

// ============================================================
//  CLI 引数
// ============================================================

/// ランダムユーザーリスト生成器
#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// 生成するユーザー件数（最大 10_000_000）
    #[arg(short, long, default_value_t = 1000)]
    count: u64,

    /// 姓 CSV ファイルパス
    #[arg(long, default_value = "data/family_name.csv")]
    family_name: PathBuf,

    /// 男性名 CSV ファイルパス
    #[arg(long, default_value = "data/given_name_male.csv")]
    given_name_male: PathBuf,

    /// 女性名 CSV ファイルパス
    #[arg(long, default_value = "data/given_name_female.csv")]
    given_name_female: PathBuf,

    /// 郵便番号 ZIP ファイルパス（utf_ken_all.zip）
    #[arg(long, default_value = "data/utf_ken_all.zip")]
    ken_all: PathBuf,

    /// 都道府県別出現頻度 CSV ファイルパス
    #[arg(long, default_value = "data/ken_frequency.csv")]
    ken_frequency: PathBuf,

    /// 出力ディレクトリ
    #[arg(short, long, default_value = "output")]
    output_dir: PathBuf,

    /// 分割ファイルあたりの最大件数
    #[arg(long, default_value_t = 1_000_000)]
    chunk_size: u64,

    /// 並列スレッド数（0 = CPU コア数を自動使用）
    #[arg(long, default_value_t = 0)]
    threads: usize,
}

// ============================================================
//  データ構造
// ============================================================

/// 姓レコード
#[derive(Debug, Clone)]
struct FamilyName {
    kanji: String,
    hiragana: String,
    romaji: String,
}

/// 名レコード（男女共用）
#[derive(Debug, Clone)]
struct GivenName {
    kanji: String,
    hiragana: String,
    romaji: String,
}

/// 重み付き選択テーブル。
/// ロード時に一度だけ構築し、選択は累積和への二分探索で O(log n)。
struct WeightedTable<T> {
    items: Vec<T>,
    cumulative: Vec<f64>, // cumulative[i] = items[0..=i] の重みの累積和
    total: f64,
}

impl<T> WeightedTable<T> {
    fn build(items: Vec<T>, weights: Vec<f64>) -> Self {
        assert_eq!(items.len(), weights.len());
        let mut cumulative = Vec::with_capacity(weights.len());
        let mut acc = 0.0f64;
        for &w in &weights {
            acc += w;
            cumulative.push(acc);
        }
        Self { items, cumulative, total: acc }
    }

    /// 重みに比例した確率で 1 件をランダムに返す。
    fn sample<'a>(&'a self, rng: &mut impl Rng) -> &'a T {
        let r = rng.gen_range(0.0..self.total);
        // 二分探索で r を超える最初のインデックスを求める
        let idx = self.cumulative.partition_point(|&c| c <= r);
        &self.items[idx.min(self.items.len() - 1)]
    }
}

/// 住所レコード（郵便番号 CSV の 1 行分）
#[derive(Debug, Clone)]
struct Address {
    postcode: String,
    prefecture: String,
    municipality: String,
    town_area: String,
}

/// 生成済みユーザー 1 件
#[derive(Debug)]
struct User {
    username: String,
    email: String,
    family_name: String,
    family_name_hiragana: String,
    family_name_romaji: String,
    given_name: String,
    given_name_hiragana: String,
    given_name_romaji: String,
    gender: u8, // 1=男, 2=女
    birth_date: String,
    postcode: String,
    prefecture_name: String,
    municipality_name: String,
    town_area_name: String,
    phone_number: String,
}

// ============================================================
//  データ読み込み
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

/// 姓 CSV を読み込む。
/// 期待するフォーマット: `kanji,hiragana,romaji,frequency`（ヘッダー行あり）
fn load_family_names(path: &PathBuf) -> Result<WeightedTable<FamilyName>> {
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
fn load_given_names(path: &PathBuf) -> Result<WeightedTable<GivenName>> {
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

/// 都道府県別出現頻度 CSV を読み込み、都道府県名 → 重み のマップを返す。
/// 期待フォーマット: `prefecture,hiragana,romaji,population,frequency`（ヘッダー行あり）
/// 重み = 1 / 2^(frequency - 1)
fn load_ken_frequency(path: &PathBuf) -> Result<HashMap<String, f64>> {
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

/// utf_ken_all.zip を展開し、都道府県別出現頻度を反映した WeightedTable を返す。
/// 内部は WeightedTable<Vec<Address>>:
///   - 第1段階: 都道府県を重み付き選択
///   - 第2段階: 選ばれた都道府県内の住所を均等選択
/// 期待フォーマット（郵便番号 CSV）:
///   col[2]  = 7桁郵便番号
///   col[6]  = 都道府県名（漢字）
///   col[7]  = 市区町村名（漢字）
///   col[8]  = 町域名（漢字）
fn load_addresses(
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
        let postcode = rec.get(2).unwrap_or("").trim().to_string();
        let prefecture = rec.get(6).unwrap_or("").trim().to_string();
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

        // 全角括弧「（」「）」のみ除去し、括弧内の文字列は残す
        let town_area = raw_town_area.replace('（', "").replace('）', "");

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
//  ユーザー生成
// ============================================================

/// 生年月日をランダム生成（12歳以上 105歳以下）
fn random_birth_date(rng: &mut impl Rng, today: NaiveDate) -> String {
    // 最年長: today - 105年
    let min_date = today - Duration::days(105 * 365 + 26); // うるう年補正
    // 最年少: today - 12年
    let max_date = today - Duration::days(12 * 365 + 3);

    let min_ord = min_date.num_days_from_ce();
    let max_ord = max_date.num_days_from_ce();
    let rand_ord = rng.gen_range(min_ord..=max_ord);
    NaiveDate::from_num_days_from_ce_opt(rand_ord)
        .map(|d| format!("{}-{:02}-{:02}", d.year(), d.month(), d.day()))
        .unwrap_or_else(|| "1990-01-01".to_string())
}

/// 電話番号インデックス（0..PHONE_SPACE）を文字列に変換する。
/// 番号空間: {050|070|080|090} × 0{000..999} × {0000..9999} = 4,000万通り
///   index = prefix_idx * 10_000_000 + mid * 10_000 + tail
///   フォーマット例） 090-0123-4567
const PHONE_SPACE: u32 = 4 * 1_000 * 10_000; // 40_000_000
const PHONE_PREFIXES: [&str; 4] = ["050", "070", "080", "090"];

fn phone_index_to_string(idx: u32) -> String {
    let tail       = idx % 10_000;
    let mid        = (idx / 10_000) % 1_000;
    let prefix_idx = (idx / 10_000_000) as usize;
    format!("{}-0{:03}-{:04}", PHONE_PREFIXES[prefix_idx], mid, tail)
}

/// 必要件数分の非重複電話番号インデックスを Fisher-Yates partial shuffle で生成する。
/// Vec<u32> として返し、チャンクはスライスで受け取るため Mutex 不要。
fn generate_phone_indices(total: usize, rng: &mut impl Rng) -> Vec<u32> {
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

/// username の重複カウンタ（スレッド間共有）
type UsernameCounter = Arc<Mutex<HashMap<String, u32>>>;

/// 重複を考慮した username を生成する
fn resolve_username(base: &str, counter: &UsernameCounter) -> String {
    let mut map = counter.lock().unwrap();
    let entry = map.entry(base.to_string()).or_insert(0);
    *entry += 1;
    if *entry == 1 {
        base.to_string()
    } else {
        format!("{}{}", base, entry)
    }
}

/// 半角数字を全角数字に変換する。
fn to_fullwidth(n: u8) -> String {
    n.to_string()
        .chars()
        .map(|c| char::from_u32('０' as u32 + (c as u32 - '0' as u32)).unwrap_or(c))
        .collect()
}

/// 丁目・番地・号をランダム生成する（数字・ハイフンはすべて全角）。
/// - depth 0: 丁目のみ          例）「３」
/// - depth 1: 丁目－番地        例）「３－７」
/// - depth 2: 丁目－番地－号    例）「３－７－１２」
fn random_street_number(rng: &mut impl Rng) -> String {
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

/// ユーザー 1 件を生成する
fn generate_user(
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
        postcode: addr.postcode.clone(),
        prefecture_name: addr.prefecture.clone(),
        municipality_name: addr.municipality.clone(),
        phone_number,
        // 丁目・番地・号の付与判定:
        // town_area に全角数字（丁目相当）または半角数字が含まれる場合はそのまま使用し、
        // それ以外はランダムな番地を末尾に付与する。
        town_area_name: {
            let ta = &addr.town_area;
            let has_number = ta.chars().any(|c| c.is_ascii_digit())
                || ta.chars().any(|c| ('０'..='９').contains(&c))
                || ta.contains('丁');
            if has_number {
                ta.clone()
            } else {
                format!("{}{}", ta, random_street_number(rng))
            }
        },
    }
}

// ============================================================
//  CSV 書き込み
// ============================================================

const CSV_HEADERS: &[&str] = &[
    "username",
    "email",
    "familyName",
    "familyNameHiragana",
    "familyNameRomaji",
    "givenName",
    "givenNameHiragana",
    "givenNameRomaji",
    "gender",
    "birthDate",
    "phoneNumber",
    "postcode",
    "prefectureName",
    "municipalityName",
    "townAreaName",
];

fn write_chunk(users: &[User], path: &PathBuf) -> Result<()> {
    let file = File::create(path)
        .with_context(|| format!("出力ファイルを作成できません: {}", path.display()))?;
    let mut wtr = Writer::from_writer(file);

    // ヘッダー
    wtr.write_record(CSV_HEADERS)?;

    for u in users {
        wtr.write_record(&[
            &u.username,
            &u.email,
            &u.family_name,
            &u.family_name_hiragana,
            &u.family_name_romaji,
            &u.given_name,
            &u.given_name_hiragana,
            &u.given_name_romaji,
            &u.gender.to_string(),
            &u.birth_date,
            &u.phone_number,
            &u.postcode,
            &u.prefecture_name,
            &u.municipality_name,
            &u.town_area_name,
        ])?;
    }
    wtr.flush()?;
    Ok(())
}

// ============================================================
//  メイン
// ============================================================

fn main() -> Result<()> {
    let args = Args::parse();

    // スレッド数設定
    if args.threads > 0 {
        rayon::ThreadPoolBuilder::new()
            .num_threads(args.threads)
            .build_global()
            .ok();
    }

    // 件数バリデーション
    let total = args.count.min(10_000_000);
    println!("生成件数: {} 件", total);

    // データ読み込み
    println!("データファイルを読み込んでいます...");
    let family_names = Arc::new(load_family_names(&args.family_name)?);
    let male_names = Arc::new(load_given_names(&args.given_name_male)?);
    let female_names = Arc::new(load_given_names(&args.given_name_female)?);
    let ken_freq = load_ken_frequency(&args.ken_frequency)?;
    let addresses = Arc::new(load_addresses(&args.ken_all, &ken_freq)?);
    let total_addr: usize = addresses.items.iter().map(|v| v.len()).sum();
    println!(
        "  姓: {} 件 / 男性名: {} 件 / 女性名: {} 件 / 都道府県: {} 件 / 住所: {} 件",
        family_names.items.len(),
        male_names.items.len(),
        female_names.items.len(),
        addresses.items.len(),
        total_addr,
    );

    // 出力ディレクトリ作成
    std::fs::create_dir_all(&args.output_dir)?;

    // 今日の日付（生年月日計算用）
    let today = Local::now().date_naive();

    // username 重複カウンタ（全スレッド共有）
    let username_counter: UsernameCounter = Arc::new(Mutex::new(HashMap::new()));

    // チャンク計算
    let chunk_size = args.chunk_size.min(total) as usize;
    let total_usize = total as usize;
    let num_chunks = (total_usize + chunk_size - 1) / chunk_size;

    // 電話番号インデックスを事前生成（全件・重複なし・Mutex 不要）
    println!("電話番号インデックスを生成しています...");
    let phone_indices: Arc<Vec<u32>> = {
        let mut rng = SmallRng::from_entropy();
        Arc::new(generate_phone_indices(total_usize, &mut rng))
    };

    println!(
        "チャンク数: {} （1チャンク最大 {} 件）",
        num_chunks, chunk_size
    );

    // ── 並列生成 & 書き込み ──────────────────────────────────
    // チャンクごとに「生成 → 書き込み」を rayon で並列実行する。
    // username 重複解決のみ Mutex で直列化し、それ以外は完全並列。

    let family_names = Arc::clone(&family_names);
    let male_names = Arc::clone(&male_names);
    let female_names = Arc::clone(&female_names);
    let addresses = Arc::clone(&addresses);
    let username_counter = Arc::clone(&username_counter);
    let phone_indices = Arc::clone(&phone_indices);
    let output_dir = args.output_dir.clone();

    let results: Vec<Result<()>> = (0..num_chunks)
        .into_par_iter()
        .map(|chunk_idx| {
            let start = chunk_idx * chunk_size;
            let end = (start + chunk_size).min(total_usize);
            let n = end - start;

            let family_names = Arc::clone(&family_names);
            let male_names = Arc::clone(&male_names);
            let female_names = Arc::clone(&female_names);
            let addresses = Arc::clone(&addresses);
            let username_counter = Arc::clone(&username_counter);
            let phone_indices = Arc::clone(&phone_indices);

            // スレッドローカル乱数生成器（高速）
            let mut rng = SmallRng::from_entropy();

            // ユーザー生成
            let users: Vec<User> = (0..n)
                .map(|i| {
                    generate_user(
                        &mut rng,
                        &family_names,
                        &male_names,
                        &female_names,
                        &addresses,
                        today,
                        &username_counter,
                        phone_indices[start + i],
                    )
                })
                .collect();

            // CSV 書き込み
            let filename = if num_chunks == 1 {
                output_dir.join("users.csv")
            } else {
                output_dir.join(format!("users_{:04}.csv", chunk_idx + 1))
            };

            write_chunk(&users, &filename)
                .with_context(|| format!("チャンク {} の書き込みに失敗", chunk_idx + 1))?;

            println!(
                "  [{}/{}] {} を書き込みました（{} 件）",
                chunk_idx + 1,
                num_chunks,
                filename.display(),
                n
            );
            Ok(())
        })
        .collect();

    // エラー集約
    let errors: Vec<_> = results.into_iter().filter_map(|r| r.err()).collect();
    if !errors.is_empty() {
        for e in &errors {
            eprintln!("エラー: {:#}", e);
        }
        anyhow::bail!("{} チャンクでエラーが発生しました", errors.len());
    }

    println!("\n✅ 完了: {} 件を {} に出力しました", total, output_dir.display());
    Ok(())
}
