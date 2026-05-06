mod generator;
mod loader;
mod models;
mod writer;

use anyhow::{Context, Result};
use clap::Parser;
use rand::rngs::SmallRng;
use rand::SeedableRng;
use rayon::prelude::*;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use generator::{
    generate_phone_indices, generate_user, UsernameCounter, PHONE_SPACE,
};
use loader::{load_addresses, load_family_names, load_given_names, load_ken_frequency};
use writer::write_chunk;

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
    let total = args.count.min(PHONE_SPACE as u64).min(10_000_000);
    println!("生成件数: {} 件", total);

    // データ読み込み
    println!("データファイルを読み込んでいます...");
    let family_names = Arc::new(load_family_names(&args.family_name)?);
    let male_names   = Arc::new(load_given_names(&args.given_name_male)?);
    let female_names = Arc::new(load_given_names(&args.given_name_female)?);
    let ken_freq     = load_ken_frequency(&args.ken_frequency)?;
    let addresses    = Arc::new(load_addresses(&args.ken_all, &ken_freq)?);
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
    let today = chrono::Local::now().date_naive();

    // username 重複カウンタ（全スレッド共有）
    let username_counter: UsernameCounter = Arc::new(Mutex::new(HashMap::new()));

    // チャンク計算
    let chunk_size  = args.chunk_size.min(total) as usize;
    let total_usize = total as usize;
    let num_chunks  = (total_usize + chunk_size - 1) / chunk_size;

    // 電話番号インデックスを事前生成（全件・重複なし・Mutex 不要）
    println!("電話番号インデックスを生成しています...");
    let phone_indices: Arc<Vec<u32>> = {
        let mut rng = SmallRng::from_entropy();
        Arc::new(generate_phone_indices(total_usize, &mut rng))
    };

    println!("チャンク数: {} （1チャンク最大 {} 件）", num_chunks, chunk_size);

    // ── 並列生成 & 書き込み ──────────────────────────────────
    let family_names    = Arc::clone(&family_names);
    let male_names      = Arc::clone(&male_names);
    let female_names    = Arc::clone(&female_names);
    let addresses       = Arc::clone(&addresses);
    let username_counter = Arc::clone(&username_counter);
    let phone_indices   = Arc::clone(&phone_indices);
    let output_dir      = args.output_dir.clone();

    let results: Vec<anyhow::Result<()>> = (0..num_chunks)
        .into_par_iter()
        .map(|chunk_idx| {
            let start = chunk_idx * chunk_size;
            let end   = (start + chunk_size).min(total_usize);
            let n     = end - start;

            let family_names     = Arc::clone(&family_names);
            let male_names       = Arc::clone(&male_names);
            let female_names     = Arc::clone(&female_names);
            let addresses        = Arc::clone(&addresses);
            let username_counter = Arc::clone(&username_counter);
            let phone_indices    = Arc::clone(&phone_indices);

            let mut rng = SmallRng::from_entropy();

            let users = (0..n)
                .map(|i| generate_user(
                    &mut rng,
                    &family_names,
                    &male_names,
                    &female_names,
                    &addresses,
                    today,
                    &username_counter,
                    phone_indices[start + i],
                ))
                .collect::<Vec<_>>();

            let filename = if num_chunks == 1 {
                output_dir.join("users.csv")
            } else {
                output_dir.join(format!("users_{:04}.csv", chunk_idx + 1))
            };

            write_chunk(&users, &filename)
                .with_context(|| format!("チャンク {} の書き込みに失敗", chunk_idx + 1))?;

            println!(
                "  [{}/{}] {} を書き込みました（{} 件）",
                chunk_idx + 1, num_chunks, filename.display(), n
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

    println!("\n✅ 完了: {} 件を {} に出力しました", total, args.output_dir.display());
    Ok(())
}
