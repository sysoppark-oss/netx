//! 말씀 표시. 자료와 규칙은 저장소의 `word/`에 있고(`word/README.md`, 기획자 확정 2026-11-11), 여기서는 그 파일을
//! 글자 하나 바꾸지 않고 빌드에 넣어 규칙대로 고르기만 한다.
//!
//! - 네트워크를 쓰지 않는다. 몇 번 보였는지 세거나 기록하지 않는다. 끄는 설정은 없다.
//! - 구절을 자르거나 줄이지 않는다. 긴 구절(`long`)은 기다리는 화면에 쓰지 않는다.
//! - 확인 창, 경고·오류, 진단 결과와 원본 출력, 감사 로그, 내보내는 파일에는 넣지 않는다(화면 쪽 `Word.tsx`만 이 자료를 쓴다).

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

const VERSES: &str = include_str!("../../word/ko.json");
const DEDICATION: &str = include_str!("../../word/dedication.ko.md");
const CHURCH: &str = include_str!("../../word/church.ko.md");

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

fn book() -> &'static Book {
    static BOOK: OnceLock<Book> = OnceLock::new();
    // 빌드에 넣은 파일이므로 읽지 못하는 일은 없다. 단위 시험이 형식을 확인한다.
    BOOK.get_or_init(|| serde_json::from_str(VERSES).unwrap_or(Book { version: String::new(), verses: Vec::new() }))
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
pub fn today(day: i64) -> Option<&'static Verse> {
    let (gospel, others): (Vec<&Verse>, Vec<&Verse>) = book().verses.iter().partition(|v| v.group == "gospel");
    if day.rem_euclid(3) == 0 { pick(&gospel, day.div_euclid(3)) } else { pick(&others, day - day.div_euclid(3) - 1) }
}

/// 기다리는 화면·빈 화면·시작 화면에 차례로 돌릴 구절(긴 구절 제외)과 오늘의 시작 위치
pub fn waiting(day: i64) -> (Vec<&'static Verse>, usize) {
    let list: Vec<&Verse> = book().verses.iter().filter(|v| !v.long).collect();
    let start = i64::try_from(list.len()).ok().filter(|n| *n > 0).and_then(|n| usize::try_from(day.rem_euclid(n)).ok()).unwrap_or(0);
    (list, start)
}

/// 교회 안내 문안에 글자로 적힌 전자우편 주소
pub fn church_email() -> Option<&'static str> {
    let addr = CHURCH.lines().find_map(|l| l.trim().strip_prefix("전자우편:"))?.trim();
    let ok = addr.len() <= 100 && addr.split_once('@').is_some_and(|(a, b)| !a.is_empty() && b.contains('.')) && addr.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'@' | b'.' | b'-' | b'_' | b'+'));
    ok.then_some(addr)
}

/// 화면에 보내는 말씀 자료
#[derive(Debug, Clone, Serialize)]
pub struct WordView {
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

/// 사용자 PC의 현지 날짜로 고른 자료
pub fn view(year: i32, month: u32, day: u32) -> Result<WordView, String> {
    let day = day_number(year, month, day).ok_or("날짜가 올바르지 않습니다")?;
    let (waiting, start) = waiting(day);
    Ok(WordView { version: &book().version, today: today(day), waiting, start, dedication: DEDICATION, church: CHURCH, email: church_email() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_verse_file_has_the_agreed_shape() {
        let b = book();
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
            assert_eq!(today(day).map(|v| v.id.as_str()), Some(id), "{y}-{m}-{d}");
            assert_eq!(view(y, m, d).unwrap().today.map(|v| v.id.as_str()), Some(id));
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
        let ids = |from: i64, n: i64| (from..from + n).filter_map(today).map(|v| (v.id.as_str(), v.group.as_str())).collect::<Vec<_>>();
        let span = ids(20768, 120);
        assert_eq!(span.len(), 120);
        for (i, (_, group)) in span.iter().enumerate() {
            assert_eq!(*group == "gospel", (20768 + i as i64) % 3 == 0, "{i}");
        }
        let mut seen: Vec<&str> = span.iter().map(|(id, _)| *id).collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 100, "120일 안에 100곳이 모두 나온다");
        assert_eq!(today(20769), today(20769 + 60), "복음 구절은 60일마다 되돌아온다");
        assert_eq!(today(20768), today(20768 + 120), "나머지 구절은 120일마다 되돌아온다");

        // 기다리는 화면: 긴 구절은 없고, 시작 위치는 day % 96
        let (list, start) = waiting(20768);
        assert_eq!((list.len(), start), (96, 20768 % 96));
        assert!(list.iter().all(|v| !v.long));
        for long in ["psa.23.1-4", "jhn.1.1-5", "rom.12.1-2", "rev.21.3-4"] {
            assert!(list.iter().all(|v| v.id != long), "{long}");
            assert!(book().verses.iter().any(|v| v.id == long && v.long));
        }
        assert_eq!(waiting(20768 + 96).1, start);
    }

    #[test]
    fn about_texts_come_from_the_files_unchanged() {
        let v = view(2026, 11, 11).unwrap();
        assert_eq!(v.dedication, include_str!("../../word/dedication.ko.md"));
        assert_eq!(v.church, include_str!("../../word/church.ko.md"));
        assert!(v.dedication.contains("사도행전 1:8") && v.church.contains("맑은물속교회"));
        // 전자우편 주소는 문안에 적힌 글자에서 읽는다.
        let email = v.email.expect("주소가 있어야 한다");
        assert!(v.church.contains(email) && email.contains('@'));
        assert!(view(2026, 2, 30).is_err());
    }
}
