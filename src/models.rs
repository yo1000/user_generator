use rand::RngExt;

// ============================================================
//  名前レコード
// ============================================================

/// 姓レコード
#[derive(Debug, Clone)]
pub struct FamilyName {
    pub kanji: String,
    pub hiragana: String,
    pub romaji: String,
}

/// 名レコード（男女共用）
#[derive(Debug, Clone)]
pub struct GivenName {
    pub kanji: String,
    pub hiragana: String,
    pub romaji: String,
}

// ============================================================
//  住所レコード
// ============================================================

/// 住所レコード（郵便番号 CSV の 1 行分）
#[derive(Debug, Clone)]
pub struct Address {
    pub postcode: String,
    pub prefecture: String,
    pub municipality: String,
    pub town_area: String,
}

// ============================================================
//  生成済みユーザー
// ============================================================

/// 生成済みユーザー 1 件
#[derive(Debug)]
pub struct User {
    pub username: String,
    pub email: String,
    pub family_name: String,
    pub family_name_hiragana: String,
    pub family_name_romaji: String,
    pub given_name: String,
    pub given_name_hiragana: String,
    pub given_name_romaji: String,
    pub gender: u8, // 1=男, 2=女
    pub birth_date: String,
    pub phone_number: String,
    pub postcode: String,
    pub prefecture_name: String,
    pub municipality_name: String,
    pub town_area_name: String,
}

// ============================================================
//  重み付き選択テーブル
// ============================================================

/// 重み付き選択テーブル。
/// ロード時に一度だけ構築し、選択は累積和への二分探索で O(log n)。
pub struct WeightedTable<T> {
    pub items: Vec<T>,
    cumulative: Vec<f64>, // cumulative[i] = items[0..=i] の重みの累積和
    total: f64,
}

impl<T> WeightedTable<T> {
    pub fn build(items: Vec<T>, weights: Vec<f64>) -> Self {
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
    pub fn sample<'a>(&'a self, rng: &mut impl RngExt) -> &'a T {
        let r = rng.random_range(0.0..self.total);
        // 二分探索で r を超える最初のインデックスを求める
        let idx = self.cumulative.partition_point(|&c| c <= r);
        &self.items[idx.min(self.items.len() - 1)]
    }
}
