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
