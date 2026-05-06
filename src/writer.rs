use std::{fs::File, path::PathBuf};

use anyhow::{Context, Result};
use csv::Writer;

use crate::models::User;

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

pub fn write_chunk(users: &[User], path: &PathBuf) -> Result<()> {
    let file = File::create(path)
        .with_context(|| format!("出力ファイルを作成できません: {}", path.display()))?;
    let mut wtr = Writer::from_writer(file);

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
//  テスト
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::User;

    // ----------------------------------------------------------
    //  ヘルパー
    // ----------------------------------------------------------

    fn make_user(username: &str, gender: u8) -> User {
        User {
            username:              username.to_string(),
            email:                 format!("{}@example.com", username),
            family_name:           "山田".to_string(),
            family_name_hiragana:  "やまだ".to_string(),
            family_name_romaji:    "Yamada".to_string(),
            given_name:            "太郎".to_string(),
            given_name_hiragana:   "たろう".to_string(),
            given_name_romaji:     "Taro".to_string(),
            gender,
            birth_date:            "1990-01-01".to_string(),
            phone_number:          "090-0123-4567".to_string(),
            postcode:              "100-0001".to_string(),
            prefecture_name:       "東京都".to_string(),
            municipality_name:     "千代田区".to_string(),
            town_area_name:        "丸の内１－２－３".to_string(),
        }
    }

    fn tempfile_path(suffix: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("writer_test_{:?}{}", std::thread::current().id(), suffix));
        path
    }

    // ----------------------------------------------------------
    //  ヘッダー
    // ----------------------------------------------------------

    #[test]
    fn test_write_chunk_header_columns() {
        let path = tempfile_path("_header.csv");
        write_chunk(&[], &path).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let header = content.lines().next().unwrap();
        let cols: Vec<&str> = header.split(',').collect();

        assert_eq!(cols[0],  "username");
        assert_eq!(cols[1],  "email");
        assert_eq!(cols[2],  "familyName");
        assert_eq!(cols[3],  "familyNameHiragana");
        assert_eq!(cols[4],  "familyNameRomaji");
        assert_eq!(cols[5],  "givenName");
        assert_eq!(cols[6],  "givenNameHiragana");
        assert_eq!(cols[7],  "givenNameRomaji");
        assert_eq!(cols[8],  "gender");
        assert_eq!(cols[9],  "birthDate");
        assert_eq!(cols[10], "phoneNumber");
        assert_eq!(cols[11], "postcode");
        assert_eq!(cols[12], "prefectureName");
        assert_eq!(cols[13], "municipalityName");
        assert_eq!(cols[14], "townAreaName");
        assert_eq!(cols.len(), 15, "列数が変わっている");
    }

    #[test]
    fn test_write_chunk_header_only_when_empty() {
        // ユーザーが 0 件でもヘッダー行は出力される
        let path = tempfile_path("_empty.csv");
        write_chunk(&[], &path).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 1, "ヘッダー行のみのはず");
    }

    // ----------------------------------------------------------
    //  データ行
    // ----------------------------------------------------------

    #[test]
    fn test_write_chunk_single_user_row_count() {
        let path = tempfile_path("_single.csv");
        let users = vec![make_user("yamada.taro", 1)];
        write_chunk(&users, &path).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        // ヘッダー + データ 1 行
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn test_write_chunk_field_values() {
        let path = tempfile_path("_fields.csv");
        let users = vec![make_user("yamada.taro", 1)];
        write_chunk(&users, &path).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let data_line = content.lines().nth(1).unwrap();
        let cols: Vec<&str> = data_line.split(',').collect();

        assert_eq!(cols[0],  "yamada.taro");
        assert_eq!(cols[1],  "yamada.taro@example.com");
        assert_eq!(cols[8],  "1");          // gender
        assert_eq!(cols[9],  "1990-01-01"); // birthDate
        assert_eq!(cols[10], "090-0123-4567"); // phoneNumber
        assert_eq!(cols[11], "100-0001");   // postcode
    }

    #[test]
    fn test_write_chunk_multiple_users() {
        let path = tempfile_path("_multi.csv");
        let users = vec![
            make_user("yamada.taro",  1),
            make_user("yamada.hanako", 2),
            make_user("suzuki.jiro",  1),
        ];
        write_chunk(&users, &path).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 4); // ヘッダー + 3 行
    }

    #[test]
    fn test_write_chunk_gender_values() {
        let path = tempfile_path("_gender.csv");
        let users = vec![make_user("user_m", 1), make_user("user_f", 2)];
        write_chunk(&users, &path).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let mut lines = content.lines().skip(1); // ヘッダーをスキップ
        let male_gender = lines.next().unwrap().split(',').nth(8).unwrap();
        let female_gender = lines.next().unwrap().split(',').nth(8).unwrap();
        assert_eq!(male_gender, "1");
        assert_eq!(female_gender, "2");
    }

    #[test]
    fn test_write_chunk_creates_file() {
        let path = tempfile_path("_create.csv");
        // 事前にファイルが存在しないことを確認
        let _ = std::fs::remove_file(&path);
        assert!(!path.exists());

        write_chunk(&[], &path).unwrap();
        assert!(path.exists(), "ファイルが作成されていない");
    }

    #[test]
    fn test_write_chunk_invalid_path_returns_err() {
        // 存在しないディレクトリへの書き込みはエラーになる
        let path = PathBuf::from("/nonexistent_dir/test.csv");
        assert!(write_chunk(&[], &path).is_err());
    }
}
