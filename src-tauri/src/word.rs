//! 말씀 표시. 자료와 규칙은 저장소의 `word/`에 있고(`word/README.md`, 기획자 확정 2026-11-11), 여기서는 그 파일을
//! 글자 하나 바꾸지 않고 빌드에 넣어 규칙대로 고르기만 한다.
//!
//! - 네트워크를 쓰지 않는다. 몇 번 보였는지 세거나 기록하지 않는다. 끄는 설정은 없다.
//! - 구절을 자르거나 줄이지 않는다. 긴 구절(`long`)은 기다리는 화면에 쓰지 않는다.
//! - 언어: `word/README.md`의 "언어 선택". 실린 언어는 `LANGS`(자료 파일이 있는 것만). 고르는 순서는
//!   내 설정(HKCU) → 설치 때 값(HKLM) → Windows 표시 언어 → `en` → `ko`. 값은 목록과 글자 비교만 한다.
//!   "오늘의 말씀"은 자리(파일 순서)로 정하므로 어느 언어에서나 같은 날 같은 곳이다. 긴 구절은 언어마다 다르다.
//! - 확인 창, 경고·오류, 진단 결과와 원본 출력, 감사 로그, 내보내는 파일에는 넣지 않는다(화면 쪽 `Word.tsx`만 이 자료를 쓴다).

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// 한 언어의 자료 묶음(파일 그대로)
struct Pack {
    lang: &'static str,
    /// 설정 화면에 보이는 이름(그 언어로)
    label: &'static str,
    verses: &'static str,
    dedication: &'static str,
    church: &'static str,
    /// 교회 안내 문안에서 전자우편 주소 앞에 붙는 말
    email_label: &'static str,
}

const PACKS: [Pack; 2] = [
    Pack {
        lang: "ko",
        label: "한국어",
        verses: include_str!("../../word/ko.json"),
        dedication: include_str!("../../word/dedication.ko.md"),
        church: include_str!("../../word/church.ko.md"),
        email_label: "전자우편:",
    },
    Pack {
        lang: "en",
        label: "English",
        verses: include_str!("../../word/en.json"),
        dedication: include_str!("../../word/dedication.en.md"),
        church: include_str!("../../word/church.en.md"),
        email_label: "Email:",
    },
];

/// 아무 값도 맞지 않을 때의 순서 (README: `en` → `ko`)
const FALLBACK: [&str; 2] = ["en", "ko"];

fn pack(lang: &str) -> &'static Pack {
    PACKS.iter().find(|p| p.lang == lang).unwrap_or(&PACKS[0])
}

/// 실린 언어 (코드, 이름)
pub fn languages() -> Vec<(&'static str, &'static str)> {
    PACKS.iter().map(|p| (p.lang, p.label)).collect()
}

/// 실린 언어면 그 코드를 돌려준다. 글자 그대로 비교만 한다.
pub fn known(lang: &str) -> Option<&'static str> {
    PACKS.iter().find(|p| p.lang == lang).map(|p| p.lang)
}

/// 말씀 언어를 고른다. `saved`는 내 설정(HKCU), `installed`는 설치 때 값(HKLM), `display`는 Windows 표시 언어("ko-KR" 등).
/// 앞의 값이 실린 언어가 아니면 다음으로 넘어간다.
pub fn choose(saved: Option<&str>, installed: Option<&str>, display: Option<&str>) -> &'static str {
    // 표시 언어는 "ko-KR", "en_US"처럼 오므로 앞부분(언어)만 본다.
    let primary = display.map(|d| d.split(['-', '_']).next().unwrap_or("").to_ascii_lowercase());
    saved
        .and_then(known)
        .or_else(|| installed.and_then(known))
        .or_else(|| primary.as_deref().and_then(known))
        .or_else(|| FALLBACK.iter().find_map(|l| known(l)))
        .unwrap_or(PACKS[0].lang)
}

/// 저장된 말씀 언어: (내 설정, 설치 때 값). Windows에서는 `SOFTWARE\Antioch`의 `WordLang`(모든 프로그램이 함께 읽는다).
#[cfg(target_os = "windows")]
pub fn stored() -> (Option<String>, Option<String>) {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    let read = |root| winreg::RegKey::predef(root).open_subkey(REG_KEY).ok().and_then(|k| k.get_value::<String, _>(REG_VALUE).ok());
    (read(HKEY_CURRENT_USER), read(HKEY_LOCAL_MACHINE))
}

/// 말씀 언어를 내 설정으로 저장한다. 실린 언어가 아니면 거절한다.
#[cfg(target_os = "windows")]
pub fn save(lang: &str) -> Result<&'static str, String> {
    let lang = known(lang).ok_or("실려 있지 않은 언어입니다")?;
    let (key, _) = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER).create_subkey(REG_KEY).map_err(|e| format!("말씀 언어를 저장하지 못했습니다: {e}"))?;
    key.set_value(REG_VALUE, &lang).map_err(|e| format!("말씀 언어를 저장하지 못했습니다: {e}"))?;
    Ok(lang)
}

#[cfg(target_os = "windows")]
const REG_KEY: &str = "SOFTWARE\\Antioch";
#[cfg(target_os = "windows")]
const REG_VALUE: &str = "WordLang";

/// Windows가 아닌 곳(개발용): 켜져 있는 동안만 기억한다.
#[cfg(not(target_os = "windows"))]
static SAVED: std::sync::Mutex<Option<&'static str>> = std::sync::Mutex::new(None);

#[cfg(not(target_os = "windows"))]
pub fn stored() -> (Option<String>, Option<String>) {
    (SAVED.lock().unwrap_or_else(|e| e.into_inner()).map(str::to_string), None)
}

#[cfg(not(target_os = "windows"))]
pub fn save(lang: &str) -> Result<&'static str, String> {
    let lang = known(lang).ok_or("실려 있지 않은 언어입니다")?;
    *SAVED.lock().unwrap_or_else(|e| e.into_inner()) = Some(lang);
    Ok(lang)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Verse {
    pub id: String,
    /// 장·절 (예: "요한복음 3:16")
    #[serde(rename = "ref")]
    pub reference: String,
    pub group: String,
    /// 긴 구절. "오늘의 말씀"의 전체 보기에서만 보인다.
    pub long: bool,
    pub text: String,
}

#[derive(Deserialize)]
struct Book {
    version: String,
    verses: Vec<Verse>,
}

fn book(lang: &str) -> &'static Book {
    static BOOKS: [OnceLock<Book>; 2] = [OnceLock::new(), OnceLock::new()];
    let index = PACKS.iter().position(|p| p.lang == lang).unwrap_or(0);
    // 빌드에 넣은 파일이므로 읽지 못하는 일은 없다. 단위 시험이 형식을 확인한다.
    BOOKS[index].get_or_init(|| serde_json::from_str(PACKS[index].verses).unwrap_or(Book { version: String::new(), verses: Vec::new() }))
}

/// 그 날짜가 1970-01-01부터 며칠째인지(1970-01-01이 0). 없는 날짜면 None.
pub fn day_number(year: i32, month: u32, day: u32) -> Option<i64> {
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let last = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    if day == 0 || day > last || !(1..=9999).contains(&year) {
        return None;
    }
    // 3월에 시작하는 해로 바꿔 세는 널리 쓰는 계산법
    let y = i64::from(year) - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (i64::from(month) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

fn pick<'a>(list: &[&'a Verse], index: i64) -> Option<&'a Verse> {
    let len = i64::try_from(list.len()).ok().filter(|n| *n > 0)?;
    list.get(usize::try_from(index.rem_euclid(len)).ok()?).copied()
}

/// 오늘의 말씀. 사흘에 한 번은 복음의 핵심 구절(`gospel`)이고, 나머지 날은 그 밖의 구절을 차례로 돈다.
pub fn today(lang: &str, day: i64) -> Option<&'static Verse> {
    let (gospel, others): (Vec<&Verse>, Vec<&Verse>) = book(lang).verses.iter().partition(|v| v.group == "gospel");
    if day.rem_euclid(3) == 0 { pick(&gospel, day.div_euclid(3)) } else { pick(&others, day - day.div_euclid(3) - 1) }
}

/// 기다리는 화면·빈 화면·시작 화면에 차례로 돌릴 구절(긴 구절 제외)과 오늘의 시작 위치
pub fn waiting(lang: &str, day: i64) -> (Vec<&'static Verse>, usize) {
    let list: Vec<&Verse> = book(lang).verses.iter().filter(|v| !v.long).collect();
    let start = i64::try_from(list.len()).ok().filter(|n| *n > 0).and_then(|n| usize::try_from(day.rem_euclid(n)).ok()).unwrap_or(0);
    (list, start)
}

/// 교회 안내 문안에 글자로 적힌 전자우편 주소
pub fn church_email(lang: &str) -> Option<&'static str> {
    let pack = pack(lang);
    let addr = pack.church.lines().find_map(|l| l.trim().strip_prefix(pack.email_label))?.trim();
    let ok = addr.len() <= 100 && addr.split_once('@').is_some_and(|(a, b)| !a.is_empty() && b.contains('.')) && addr.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'@' | b'.' | b'-' | b'_' | b'+'));
    ok.then_some(addr)
}

/// 화면에 보내는 말씀 자료
#[derive(Debug, Clone, Serialize)]
pub struct WordView {
    /// 지금 쓰는 말씀 언어
    pub lang: &'static str,
    /// 고를 수 있는 언어 (코드, 이름)
    pub languages: Vec<(&'static str, &'static str)>,
    /// 번역본 이름
    pub version: &'static str,
    pub today: Option<&'static Verse>,
    pub waiting: Vec<&'static Verse>,
    pub start: usize,
    /// "정보" 화면의 헌정 문구(파일 그대로)
    pub dedication: &'static str,
    /// [교회 안내] 문안(파일 그대로)
    pub church: &'static str,
    pub email: Option<&'static str>,
}

/// 사용자 PC의 현지 날짜로 고른 자료. `lang`이 실린 언어가 아니면 `choose`의 마지막 순서를 따른다.
pub fn view(lang: &str, year: i32, month: u32, day: u32) -> Result<WordView, String> {
    let lang = known(lang).unwrap_or_else(|| choose(None, None, None));
    let day = day_number(year, month, day).ok_or("날짜가 올바르지 않습니다")?;
    let (waiting, start) = waiting(lang, day);
    let pack = pack(lang);
    Ok(WordView { lang, languages: languages(), version: &book(lang).version, today: today(lang, day), waiting, start, dedication: pack.dedication, church: pack.church, email: church_email(lang) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_verse_file_has_the_agreed_shape() {
        let b = book("ko");
        assert_eq!(b.version, "성경전서 개역한글판(침례 표기)");
        assert_eq!(b.verses.len(), 100);
        assert_eq!(b.verses.iter().filter(|v| v.group == "gospel").count(), 20);
        assert_eq!(b.verses.iter().filter(|v| !v.long).count(), 96);
        assert!(b.verses.iter().all(|v| !v.id.is_empty() && !v.reference.is_empty() && !v.text.trim().is_empty()));
        let mut ids: Vec<&str> = b.verses.iter().map(|v| v.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 100, "id가 겹치지 않는다");
    }

    /// `word/README.md`의 확인 값
    #[test]
    fn todays_word_matches_the_readme_table() {
        for (y, m, d, day, id) in [
            (2026, 11, 11, 20768, "jhn.7.37-38"),
            (2026, 11, 12, 20769, "rom.6.23"),
            (2026, 11, 13, 20770, "jhn.8.12"),
            (2026, 11, 14, 20771, "jhn.10.9-11"),
            (2026, 12, 25, 20812, "jhn.14.27"),
            (2027, 1, 1, 20819, "psa.119.105"),
            (2028, 2, 29, 21243, "rom.3.23-24"),
        ] {
            assert_eq!(day_number(y, m, d), Some(day), "{y}-{m}-{d}");
            for lang in ["ko", "en"] {
                assert_eq!(today(lang, day).map(|v| v.id.as_str()), Some(id), "{lang} {y}-{m}-{d}");
                assert_eq!(view(lang, y, m, d).unwrap().today.map(|v| v.id.as_str()), Some(id));
            }
        }
    }

    #[test]
    fn day_numbers_follow_the_calendar() {
        assert_eq!(day_number(1970, 1, 1), Some(0));
        assert_eq!(day_number(1970, 1, 2), Some(1));
        assert_eq!(day_number(1969, 12, 31), Some(-1));
        assert_eq!(day_number(2000, 3, 1), Some(11017));
        assert_eq!(day_number(2024, 2, 29).zip(day_number(2024, 3, 1)).map(|(a, b)| b - a), Some(1));
        for bad in [(2026, 0, 1), (2026, 13, 1), (2026, 2, 29), (2026, 4, 31), (2026, 1, 0), (0, 1, 1)] {
            assert_eq!(day_number(bad.0, bad.1, bad.2), None, "{bad:?}");
        }
    }

    #[test]
    fn rotation_rules() {
        // 사흘에 한 번은 복음의 핵심 구절, 나머지 이틀은 아니다. 60일이면 복음 20곳을, 120일이면 나머지 80곳을 한 바퀴 돈다.
        let ids = |from: i64, n: i64| (from..from + n).filter_map(|d| today("ko", d)).map(|v| (v.id.as_str(), v.group.as_str())).collect::<Vec<_>>();
        let span = ids(20768, 120);
        assert_eq!(span.len(), 120);
        for (i, (_, group)) in span.iter().enumerate() {
            assert_eq!(*group == "gospel", (20768 + i as i64) % 3 == 0, "{i}");
        }
        let mut seen: Vec<&str> = span.iter().map(|(id, _)| *id).collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 100, "120일 안에 100곳이 모두 나온다");
        assert_eq!(today("ko", 20769), today("ko", 20769 + 60), "복음 구절은 60일마다 되돌아온다");
        assert_eq!(today("ko", 20768), today("ko", 20768 + 120), "나머지 구절은 120일마다 되돌아온다");

        // 기다리는 화면: 긴 구절은 없고, 시작 위치는 day % 96
        let (list, start) = waiting("ko", 20768);
        assert_eq!((list.len(), start), (96, 20768 % 96));
        assert!(list.iter().all(|v| !v.long));
        for long in ["psa.23.1-4", "jhn.1.1-5", "rom.12.1-2", "rev.21.3-4"] {
            assert!(list.iter().all(|v| v.id != long), "{long}");
            assert!(book("ko").verses.iter().any(|v| v.id == long && v.long));
        }
        assert_eq!(waiting("ko", 20768 + 96).1, start);
    }

    #[test]
    fn about_texts_come_from_the_files_unchanged() {
        let v = view("ko", 2026, 11, 11).unwrap();
        assert_eq!(v.dedication, include_str!("../../word/dedication.ko.md"));
        assert_eq!(v.church, include_str!("../../word/church.ko.md"));
        assert!(v.dedication.contains("사도행전 1:8") && v.church.contains("맑은물속교회"));
        // 전자우편 주소는 문안에 적힌 글자에서 읽는다.
        let email = v.email.expect("주소가 있어야 한다");
        assert!(v.church.contains(email) && email.contains('@'));
        assert!(view("ko", 2026, 2, 30).is_err());
    }

    #[test]
    fn the_english_file_matches_the_korean_one_place_for_place() {
        let (ko, en) = (book("ko"), book("en"));
        assert_eq!(en.version, "World English Bible");
        assert_eq!(en.verses.len(), 100);
        let ids = |b: &'static Book| b.verses.iter().map(|v| (v.id.as_str(), v.group.as_str())).collect::<Vec<_>>();
        assert_eq!(ids(ko), ids(en), "같은 id 100곳이 같은 순서, 같은 묶음");
        assert!(en.verses.iter().all(|v| !v.reference.is_empty() && !v.text.trim().is_empty()));
        // 긴 구절은 언어마다 다르다. 기다리는 화면에는 그 언어의 긴 구절을 쓰지 않는다.
        let long: Vec<&str> = en.verses.iter().filter(|v| v.long).map(|v| v.id.as_str()).collect();
        assert_eq!(long, ["psa.23.1-4", "jhn.1.1-5", "mat.28.18-20", "rom.10.14-15", "rev.7.9-10", "rom.12.1-2", "rev.21.3-4"]);
        let (list, start) = waiting("en", 20768);
        assert_eq!((list.len(), start), (93, 20768 % 93));
        assert!(list.iter().all(|v| !v.long));
        // 긴 구절도 "오늘의 말씀"으로는 온전히 나온다(자르지 않는다).
        let day = (0..400).find(|d| today("en", *d).is_some_and(|v| v.id == "mat.28.18-20")).expect("언젠가는 나온다");
        let whole = today("en", day).unwrap();
        assert!(whole.long && whole.text == en.verses.iter().find(|v| v.id == "mat.28.18-20").unwrap().text);
        assert_eq!(today("ko", day).map(|v| v.id.as_str()), Some("mat.28.18-20"), "같은 날 같은 곳");
    }

    #[test]
    fn english_texts_come_from_the_files_unchanged() {
        let v = view("en", 2026, 11, 14).unwrap();
        assert_eq!((v.lang, v.version), ("en", "World English Bible"));
        assert_eq!(v.dedication, include_str!("../../word/dedication.en.md"));
        assert_eq!(v.church, include_str!("../../word/church.en.md"));
        assert!(v.dedication.contains("Acts 1:8 (World English Bible)") && v.church.contains("Living Waters Church"));
        let today = v.today.unwrap();
        assert_eq!((today.id.as_str(), today.reference.as_str()), ("jhn.10.9-11", "John 10:9-11"));
        assert!(include_str!("../../word/en.json").contains(&serde_json::to_string(&today.text).unwrap()), "본문은 파일의 글자 그대로");
        assert_eq!(v.email, view("ko", 2026, 11, 14).unwrap().email, "두 문안의 주소가 같다");
        assert!(v.church.contains(v.email.unwrap()));
        assert_eq!(v.languages, [("ko", "한국어"), ("en", "English")]);
        // 모르는 언어 값은 영어로(README: 마지막 순서는 en → ko)
        assert_eq!(view("fr", 2026, 11, 14).unwrap().lang, "en");
    }

    #[test]
    fn language_is_chosen_in_the_readme_order() {
        // 내 설정 → 설치 때 값 → Windows 표시 언어 → en → ko
        assert_eq!(choose(Some("ko"), Some("en"), Some("en-US")), "ko");
        assert_eq!(choose(None, Some("en"), Some("ko-KR")), "en");
        assert_eq!(choose(None, None, Some("ko-KR")), "ko");
        assert_eq!(choose(None, None, Some("en_GB")), "en");
        assert_eq!(choose(None, None, Some("KO")), "ko");
        assert_eq!(choose(None, None, Some("ja-JP")), "en", "실리지 않은 언어는 영어로");
        assert_eq!(choose(None, None, None), "en");
        // 목록에 없는 값은 건너뛴다. 글자 그대로만 비교한다(경로나 명령으로 쓰지 않는다).
        assert_eq!(choose(Some("fr"), Some("ko"), None), "ko");
        assert_eq!(choose(Some("KO"), Some("..\\en"), Some("ko")), "ko");
        assert_eq!(choose(Some("en "), Some(""), Some("")), "en");
        for bad in ["", "EN", "en-US", "../ko", "ko\0", "none", "off"] {
            assert_eq!(known(bad), None, "{bad:?}");
            assert!(save(bad).is_err(), "{bad:?}");
        }
        assert_eq!(known("en"), Some("en"));
    }
}
